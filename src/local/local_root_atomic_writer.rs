// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Streaming descriptor-relative atomic file replacement.
// qubit-style: allow coverage-cfg

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::fs::File;
use std::io::{
    self,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};
use std::time::Duration;

#[cfg(unix)]
use crate::LocalRelativePath;
use crate::{
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
};

#[cfg(coverage)]
use super::internal::coverage_fault;
#[cfg(unix)]
use super::internal::{
    AtomicStagingState,
    RootedParentMode,
    RootedStagedFile,
    create_rooted_staged_file,
    inspect_rooted_atomic_destination,
    install_rooted_atomic_file,
    open_rooted_atomic_destination,
    open_rooted_parent,
    preserve_atomic_metadata,
    verify_rooted_atomic_destination_identity,
};

/// A streaming atomic writer contained by an open [`crate::LocalRoot`].
///
/// Staging, replacement, synchronization, and cleanup use the destination
/// parent descriptor and entry names. No diagnostic path is reused as
/// authority, and no underlying file or directory handle is exposed.
/// Commit opens the current destination, copies its strict platform-native
/// Unix metadata to staging, and verifies the opened file identity immediately
/// before replacement. Metadata is therefore captured at commit time rather
/// than when the writer begins. A metadata or ACL copy failure aborts instead
/// of silently reducing protection. A destination that was initially absent is
/// installed without replacing a concurrent creator.
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::{LocalRelativePath, LocalRoot};
///
/// let root = LocalRoot::open(".").unwrap();
/// let path = LocalRelativePath::new("result.bin").unwrap();
/// let writer = root.begin_atomic_write(&path).unwrap();
/// writer;
/// ```
#[must_use = "rooted atomic writes have no effect unless committed"]
#[derive(Debug)]
pub struct LocalRootAtomicWriter {
    /// Requested relative destination retained for structured errors.
    path: PathBuf,
    /// Optional limit for retrying a nonblocking destination open.
    open_retry_timeout: Option<Duration>,
    #[cfg(unix)]
    /// Final destination entry name within the staging parent.
    final_name: CString,
    #[cfg(unix)]
    /// Parents whose newly created child entries require synchronization.
    parent_dirs_to_sync: Vec<File>,
    #[cfg(unix)]
    /// Whether a regular destination existed when this writer began.
    destination_existed: bool,
    #[cfg(unix)]
    /// Descriptor-relative staging lifecycle.
    staged_file: RootedStagedFile,
}

impl LocalRootAtomicWriter {
    #[cfg(unix)]
    /// Creates a rooted atomic writer from an open root capability.
    ///
    /// # Parameters
    ///
    /// * `root` - Open root directory authority.
    /// * `diagnostic_root` - Path used only to contextualize traversal errors.
    /// * `path` - Validated relative destination.
    ///
    /// # Returns
    ///
    /// An armed rooted atomic writer.
    ///
    /// # Errors
    ///
    /// Returns a structured error for parent preparation, destination
    /// inspection, or staging-file creation failures.
    pub(crate) fn new(
        root: &File,
        diagnostic_root: &Path,
        path: &LocalRelativePath,
    ) -> Result<Self, LocalAtomicWriteError> {
        let requested_path = path.as_path().to_path_buf();
        let diagnostic_path = diagnostic_root.join(path.as_path());
        let rooted_parent = map_atomic_error(
            open_rooted_parent(
                root,
                &diagnostic_path,
                path,
                RootedParentMode::CreateMissingAndTrackSync,
            ),
            LocalAtomicWriteStage::PrepareParent,
            &requested_path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        let (parent, final_name, parent_dirs_to_sync) =
            rooted_parent.into_parts();
        let destination_existed = map_atomic_error(
            inspect_rooted_atomic_destination(&parent, &final_name),
            LocalAtomicWriteStage::InspectDestination,
            &requested_path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        let relative_parent = path.as_path().parent().unwrap_or(Path::new(""));
        let staged_file = map_atomic_error(
            create_rooted_staged_file(parent, relative_parent),
            LocalAtomicWriteStage::CreateTemporaryFile,
            &requested_path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        Ok(Self {
            path: requested_path,
            open_retry_timeout: None,
            final_name,
            parent_dirs_to_sync,
            destination_existed,
            staged_file,
        })
    }

    /// Returns the configured nonblocking-open retry timeout.
    ///
    /// On Unix, this limits how long commit waits for an existing destination
    /// whose active file lease makes a nonblocking open return
    /// [`io::ErrorKind::WouldBlock`]. `None` preserves the default unbounded
    /// wait.
    ///
    /// # Returns
    /// The configured timeout, or `None` when retries are unbounded.
    #[must_use]
    #[inline(always)]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Sets the nonblocking-open retry timeout.
    ///
    /// On Unix, [`Duration::ZERO`] returns [`io::ErrorKind::TimedOut`] after
    /// the first lease-conflicting open attempt. Other open errors are never
    /// retried.
    ///
    /// # Parameters
    /// - `timeout`: Maximum time to retry a lease-conflicting open.
    ///
    /// # Returns
    /// This writer with the timeout configured.
    #[inline(always)]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }

    /// Synchronizes and atomically replaces the rooted destination.
    ///
    /// Existing metadata is read from the opened destination during this call
    /// and applied to staging before the identity check and replacement.
    ///
    /// # Errors
    ///
    /// Returns a structured error when metadata preservation, staging-file
    /// synchronization, replacement, or parent-directory synchronization
    /// fails. Inspect [`LocalAtomicWriteError::destination_state`] to determine
    /// the known post-failure destination outcome.
    #[cfg_attr(not(unix), allow(unused_mut))]
    pub fn commit(mut self) -> Result<(), LocalAtomicWriteError> {
        #[cfg(unix)]
        {
            let destination = if self.destination_existed {
                let destination_result = open_rooted_atomic_destination(
                    self.staged_file.parent(),
                    &self.final_name,
                    self.open_retry_timeout,
                );
                let opened = with_staging_cleanup(
                    destination_result,
                    LocalAtomicWriteStage::ReadDestinationMetadata,
                    &self.path,
                    &mut self.staged_file,
                )?;
                match opened {
                    Some(destination) => Some(destination),
                    None => {
                        return Err(rooted_error_with_staging_state(
                            LocalAtomicWriteStage::ReadDestinationMetadata,
                            &self.path,
                            io::Error::new(
                                io::ErrorKind::NotFound,
                                "rooted atomic destination disappeared",
                            ),
                            LocalAtomicDestinationState::Missing,
                            &mut self.staged_file,
                        ));
                    }
                }
            } else {
                None
            };
            if let Some(destination) = destination.as_ref() {
                let result = preserve_atomic_metadata(
                    destination.file(),
                    self.staged_file.file(),
                );
                with_staging_cleanup(
                    result,
                    LocalAtomicWriteStage::ApplyDestinationMetadata,
                    &self.path,
                    &mut self.staged_file,
                )?;
            }
            let result = self.staged_file.file().sync_all();
            with_staging_cleanup(
                result,
                LocalAtomicWriteStage::SyncTemporaryFile,
                &self.path,
                &mut self.staged_file,
            )?;
            if let Some(destination) = destination.as_ref() {
                verify_rooted_atomic_destination_identity(
                    &self.final_name,
                    destination,
                    &self.path,
                    &mut self.staged_file,
                )?;
            }
            let install_result = install_rooted_atomic_file(
                &mut self.staged_file,
                &self.final_name,
                self.destination_existed,
            );
            if let Err((source, destination_state, staging_state)) =
                install_result
            {
                return recover_rooted_atomic_install_error(
                    &self.path,
                    &self.parent_dirs_to_sync,
                    source,
                    destination_state,
                    staging_state,
                    &mut self.staged_file,
                );
            }
            let temporary_path =
                self.staged_file.diagnostic_path().to_path_buf();
            self.staged_file.disarm();
            map_atomic_error(
                sync_rooted_parent_chain(
                    self.staged_file.parent(),
                    &self.parent_dirs_to_sync,
                ),
                LocalAtomicWriteStage::SyncParent,
                &self.path,
                Some(temporary_path),
                LocalAtomicDestinationState::Replaced,
            )
        }
        #[cfg(not(unix))]
        {
            Err(unsupported_atomic_error(&self.path))
        }
    }

    /// Aborts replacement and removes the descriptor-relative staging entry.
    ///
    /// # Errors
    ///
    /// Returns a structured cleanup error when `unlinkat` fails.
    #[cfg_attr(not(unix), allow(unused_mut))]
    pub fn abort(mut self) -> Result<(), LocalAtomicWriteError> {
        #[cfg(unix)]
        {
            let temporary_path =
                self.staged_file.diagnostic_path().to_path_buf();
            match self.staged_file.cleanup() {
                Ok(()) => Ok(()),
                Err(source) => Err(LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::CleanupTemporaryFile,
                    self.path.clone(),
                    Some(temporary_path),
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )),
            }
        }
        #[cfg(not(unix))]
        {
            Err(unsupported_atomic_error(&self.path))
        }
    }
}

impl Write for LocalRootAtomicWriter {
    /// Writes bytes into the private rooted staging file.
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            self.staged_file.file_mut().write(buffer)
        }
        #[cfg(not(unix))]
        {
            let _ = buffer;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "secure rooted atomic writes are unsupported on this target",
            ))
        }
    }

    /// Writes bytes from multiple buffers into the rooted staging file.
    #[inline(always)]
    fn write_vectored(
        &mut self,
        buffers: &[io::IoSlice<'_>],
    ) -> io::Result<usize> {
        #[cfg(unix)]
        {
            self.staged_file.file_mut().write_vectored(buffers)
        }
        #[cfg(not(unix))]
        {
            let _ = buffers;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "secure rooted atomic writes are unsupported on this target",
            ))
        }
    }

    /// Flushes userspace data into the private rooted staging file.
    #[inline(always)]
    fn flush(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.staged_file.file_mut().flush()
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "secure rooted atomic writes are unsupported on this target",
            ))
        }
    }
}

/// Synchronizes the final parent and newly created ancestor entries.
///
/// # Parameters
///
/// * `parent` - Final destination parent descriptor.
/// * `parent_dirs_to_sync` - Ancestor descriptors ordered shallowest to
///   deepest.
///
/// # Errors
///
/// Returns the first directory synchronization error.
#[cfg(unix)]
fn sync_rooted_parent_chain(
    parent: &File,
    parent_dirs_to_sync: &[File],
) -> io::Result<()> {
    #[cfg(coverage)]
    if coverage_fault::is_enabled("atomic-install-unlink-recover-sync")
        || coverage_fault::is_enabled("atomic-install-unlink-persistent-sync")
        || coverage_fault::is_enabled(
            "atomic-install-unlink-indeterminate-sync",
        )
    {
        return Err(io::Error::from_raw_os_error(libc::EIO));
    }
    parent.sync_all()?;
    for directory in parent_dirs_to_sync.iter().rev() {
        directory.sync_all()?;
    }
    Ok(())
}

#[cfg(unix)]
/// Adds structured atomic context to a native I/O result.
///
/// # Parameters
///
/// * `result` - Native result to map.
/// * `stage` - Atomic stage associated with failure.
/// * `path` - Requested relative destination.
/// * `temporary_path` - Optional diagnostic staging path.
/// * `destination_state` - Known destination state after the failure.
///
/// # Returns
///
/// The successful value or a structured atomic error.
fn map_atomic_error<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    temporary_path: Option<PathBuf>,
    destination_state: LocalAtomicDestinationState,
) -> Result<T, LocalAtomicWriteError> {
    match result {
        Ok(value) => Ok(value),
        Err(source) => Err(LocalAtomicWriteError::new(
            stage,
            path.to_path_buf(),
            temporary_path,
            destination_state,
            source,
        )),
    }
}

#[cfg(unix)]
/// Maps a staging failure and attempts explicit cleanup.
///
/// # Parameters
///
/// * `result` - Staging operation result to map.
/// * `stage` - Atomic stage associated with failure.
/// * `path` - Requested relative destination.
/// * `staged_file` - Armed staging guard to clean on failure.
///
/// # Returns
///
/// The successful value or a structured error retaining any cleanup failure.
fn with_staging_cleanup<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    staged_file: &mut RootedStagedFile,
) -> Result<T, LocalAtomicWriteError> {
    match result {
        Ok(value) => Ok(value),
        Err(source) => Err(rooted_error_with_staging_state(
            stage,
            path,
            source,
            LocalAtomicDestinationState::Unchanged,
            staged_file,
        )),
    }
}

#[cfg(unix)]
/// Creates a rooted error and handles staging according to destination state.
fn rooted_error_with_staging_state(
    stage: LocalAtomicWriteStage,
    path: &Path,
    source: io::Error,
    destination_state: LocalAtomicDestinationState,
    staged_file: &mut RootedStagedFile,
) -> LocalAtomicWriteError {
    let temporary_path = staged_file.diagnostic_path().to_path_buf();
    let cleanup_error =
        if destination_state == LocalAtomicDestinationState::Unchanged {
            staged_file.cleanup().err()
        } else {
            staged_file.close();
            staged_file.disarm();
            None
        };
    LocalAtomicWriteError::new(
        stage,
        path.to_path_buf(),
        Some(temporary_path),
        destination_state,
        source,
    )
    .with_cleanup_error(cleanup_error)
}

#[cfg(unix)]
/// Recovers or reports a failed rooted destination installation.
///
/// # Parameters
///
/// * `path` - Requested relative destination retained for diagnostics.
/// * `parent_dirs_to_sync` - Newly created ancestor entries requiring sync.
/// * `source` - Primary native installation error.
/// * `destination_state` - Known destination state after installation.
/// * `staging_state` - Known staging-name state after installation.
/// * `staged_file` - Armed rooted staging guard used for recovery.
///
/// # Errors
///
/// Returns the installation error when recovery is incomplete, or a parent
/// synchronization error after a fully recovered published destination.
fn recover_rooted_atomic_install_error(
    path: &Path,
    parent_dirs_to_sync: &[File],
    source: io::Error,
    destination_state: LocalAtomicDestinationState,
    staging_state: AtomicStagingState,
    staged_file: &mut RootedStagedFile,
) -> Result<(), LocalAtomicWriteError> {
    let temporary_path = staged_file.diagnostic_path().to_path_buf();
    let cleanup_error = match staging_state {
        AtomicStagingState::Present => match staged_file.cleanup() {
            Ok(())
                if destination_state
                    == LocalAtomicDestinationState::Replaced =>
            {
                return map_atomic_error(
                    sync_rooted_parent_chain(
                        staged_file.parent(),
                        parent_dirs_to_sync,
                    ),
                    LocalAtomicWriteStage::SyncParent,
                    path,
                    Some(temporary_path),
                    destination_state,
                );
            }
            Ok(()) => None,
            Err(error) => Some(error),
        },
        AtomicStagingState::Indeterminate => {
            staged_file.close();
            staged_file.disarm();
            None
        }
    };
    let parent_sync_error =
        if destination_state == LocalAtomicDestinationState::Replaced {
            sync_rooted_parent_chain(staged_file.parent(), parent_dirs_to_sync)
                .err()
        } else {
            None
        };
    Err(LocalAtomicWriteError::new(
        LocalAtomicWriteStage::ReplaceDestination,
        path.to_path_buf(),
        Some(temporary_path),
        destination_state,
        source,
    )
    .with_cleanup_error(cleanup_error)
    .with_parent_sync_error(parent_sync_error))
}

#[cfg(not(unix))]
/// Creates a structured unsupported rooted atomic-write error.
///
/// # Parameters
///
/// * `path` - Requested relative destination.
///
/// # Returns
///
/// An unsupported error that never falls back to ordinary path authority.
fn unsupported_atomic_error(path: &Path) -> LocalAtomicWriteError {
    LocalAtomicWriteError::new(
        LocalAtomicWriteStage::PrepareParent,
        path.to_path_buf(),
        None,
        LocalAtomicDestinationState::Unchanged,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "secure rooted atomic writes are unsupported on this target",
        ),
    )
}
