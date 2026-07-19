// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Streaming durable atomic file replacement.

use std::fs;
use std::io::{
    self,
    ErrorKind,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};
use std::time::Duration;

use crate::{
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
};

use super::internal::{
    DEFAULT_TEMP_ENTRY_RETRIES,
    StagedFile,
    absolute_path,
    add_path_context,
    create_temp_file_in_dir,
    ensure_parent_path_with_sync_dirs,
    install_atomic_file,
    parent_dir_for,
    sync_parent_dir,
};
#[cfg(unix)]
use super::internal::{
    open_atomic_destination,
    preserve_atomic_metadata,
    verify_atomic_destination_identity,
};

/// Default suffix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_SUFFIX: &str = ".tmp";

/// Prefix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_PREFIX: &str = ".atomic-write-";

/// A streaming same-directory atomic file writer.
///
/// Bytes written through [`Write`] remain in a private staging file until
/// [`Self::commit`] succeeds. Calling [`Self::abort`] or dropping the writer
/// leaves the destination unchanged and removes the staging file on a
/// best-effort basis. This initial API intentionally does not implement
/// [`std::io::Seek`]. A relative destination is bound to the process current
/// directory when the writer is created, so later current-directory changes
/// do not redirect commit or cleanup operations.
///
/// The destination must be absent or a regular file when this writer is
/// created. Symbolic links, directories, sockets, FIFOs, devices, and other
/// special files are rejected. On Unix, commit opens the current destination,
/// copies its strict platform-native metadata to staging, and verifies the
/// opened file identity immediately before replacement. On Windows,
/// `ReplaceFileW` merges the existing destination metadata during replacement.
/// A metadata read, copy, ACL merge, or attribute merge failure aborts the
/// operation instead of silently reducing the protection of the destination.
/// A destination that was absent when this writer began is installed with a
/// native no-replace operation, so a concurrent creator is not overwritten.
///
/// The final inspection and replacement remain separate path-based operations.
/// This writer is therefore not a sandbox boundary against an actor that can
/// replace path entries concurrently. Use [`crate::LocalRoot`] when filesystem
/// containment must be anchored to an opened directory capability.
///
/// The guard must be committed or explicitly aborted:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::LocalFiles;
///
/// let writer = LocalFiles::begin_atomic_write("result.bin")?;
/// writer;
/// # Ok::<(), qubit_local_files::LocalAtomicWriteError>(())
/// ```
#[must_use = "atomic writes have no effect unless the writer is committed"]
#[derive(Debug)]
pub struct LocalAtomicWriter {
    /// Requested destination path.
    path: PathBuf,
    /// Absolute destination path used by filesystem operations.
    operation_path: PathBuf,
    /// Newly created parent directories that require synchronization.
    parent_dirs_to_sync: Vec<PathBuf>,
    /// Whether a regular destination existed when this writer began.
    destination_existed: bool,
    /// Optional limit for retrying a nonblocking destination open.
    open_retry_timeout: Option<Duration>,
    /// Owned same-directory staging file.
    staged_file: StagedFile,
}

impl LocalAtomicWriter {
    /// Creates a streaming atomic writer for `path`.
    ///
    /// # Parameters
    /// - `path`: Destination path to replace on commit.
    ///
    /// # Errors
    /// Returns a structured error when parent preparation, destination
    /// inspection, or staging-file creation fails.
    pub(crate) fn new(path: &Path) -> Result<Self, LocalAtomicWriteError> {
        let operation_path = with_atomic_context(
            absolute_path(path),
            LocalAtomicWriteStage::PrepareParent,
            path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        let parent_dirs_to_sync = with_atomic_context(
            ensure_parent_path_with_sync_dirs(&operation_path),
            LocalAtomicWriteStage::PrepareParent,
            path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        let destination_existed = with_atomic_context(
            existing_file_metadata(&operation_path),
            LocalAtomicWriteStage::InspectDestination,
            path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?
        .is_some();
        let parent = parent_dir_for(&operation_path);
        let (temp_path, file) = with_atomic_context(
            create_temp_file_in_dir(
                parent,
                Some(ATOMIC_WRITE_TEMP_PREFIX),
                Some(ATOMIC_WRITE_TEMP_SUFFIX),
                DEFAULT_TEMP_ENTRY_RETRIES,
            ),
            LocalAtomicWriteStage::CreateTemporaryFile,
            path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            operation_path,
            parent_dirs_to_sync,
            destination_existed,
            open_retry_timeout: None,
            staged_file: StagedFile::new(temp_path, file),
        })
    }

    /// Returns the configured nonblocking-open retry timeout.
    ///
    /// On Unix, this limits how long commit waits for an existing destination
    /// whose active file lease makes a nonblocking open return
    /// [`ErrorKind::WouldBlock`]. `None` preserves the default unbounded wait.
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
    /// On Unix, [`Duration::ZERO`] returns [`ErrorKind::TimedOut`] after the
    /// first lease-conflicting open attempt. Other open errors are never
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

    /// Synchronizes and atomically replaces the destination.
    ///
    /// Existing Unix metadata is read from an opened destination during this
    /// call, not when the writer was created. Windows delegates strict metadata
    /// merging to `ReplaceFileW`. Immediately before replacement, commit
    /// verifies the applicable destination type and identity rules.
    ///
    /// # Errors
    /// Returns a structured error when metadata preservation, staging-file
    /// synchronization, destination replacement, or parent synchronization
    /// fails. Inspect [`LocalAtomicWriteError::destination_state`] before
    /// deciding whether the destination or retained staging path needs
    /// recovery.
    pub fn commit(mut self) -> Result<(), LocalAtomicWriteError> {
        #[cfg(unix)]
        let destination = if self.destination_existed {
            let opened = with_staging_cleanup(
                open_atomic_destination(
                    &self.operation_path,
                    self.open_retry_timeout,
                ),
                LocalAtomicWriteStage::ReadDestinationMetadata,
                &self.path,
                &mut self.staged_file,
            )?;
            match opened {
                Some(destination) => Some(destination),
                None => {
                    return Err(atomic_error_with_staging_state(
                        LocalAtomicWriteStage::ReadDestinationMetadata,
                        &self.path,
                        io::Error::new(
                            ErrorKind::NotFound,
                            "atomic write destination disappeared",
                        ),
                        LocalAtomicDestinationState::Missing,
                        &mut self.staged_file,
                    ));
                }
            }
        } else {
            None
        };
        #[cfg(unix)]
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
        #[cfg(not(any(unix, windows)))]
        if self.destination_existed {
            let result = Err(io::Error::new(
                ErrorKind::Unsupported,
                "strict atomic metadata preservation is unsupported on this target",
            ));
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

        #[cfg(unix)]
        if let Some(destination) = destination.as_ref() {
            verify_atomic_destination_identity(
                &self.operation_path,
                destination,
                &self.path,
                &mut self.staged_file,
            )?;
        }
        #[cfg(not(unix))]
        if self.destination_existed {
            let metadata = with_staging_cleanup(
                existing_file_metadata(&self.operation_path),
                LocalAtomicWriteStage::ReplaceDestination,
                &self.path,
                &mut self.staged_file,
            )?;
            if metadata.is_none() {
                return Err(atomic_error_with_staging_state(
                    LocalAtomicWriteStage::ReplaceDestination,
                    &self.path,
                    io::Error::new(
                        ErrorKind::NotFound,
                        "atomic write destination disappeared",
                    ),
                    LocalAtomicDestinationState::Missing,
                    &mut self.staged_file,
                ));
            }
        }

        self.staged_file.close();
        let install_result = install_atomic_file(
            self.staged_file.path(),
            &self.operation_path,
            self.destination_existed,
        );
        if let Err((source, destination_state)) = install_result {
            return Err(atomic_error_with_staging_state(
                LocalAtomicWriteStage::ReplaceDestination,
                &self.path,
                source,
                destination_state,
                &mut self.staged_file,
            ));
        }
        let temporary_path = self.staged_file.path().to_path_buf();
        self.staged_file.disarm();
        with_atomic_context(
            sync_atomic_parent_chain(
                &self.operation_path,
                &self.parent_dirs_to_sync,
            ),
            LocalAtomicWriteStage::SyncParent,
            &self.path,
            Some(temporary_path),
            LocalAtomicDestinationState::Replaced,
        )
    }

    /// Aborts the staged replacement and removes its temporary file.
    ///
    /// The destination remains unchanged. If explicit cleanup fails, the
    /// internal staging guard retries best-effort cleanup as this method exits.
    ///
    /// # Errors
    /// Returns a structured cleanup error when the temporary file cannot be
    /// removed.
    pub fn abort(mut self) -> Result<(), LocalAtomicWriteError> {
        let temporary_path = self.staged_file.path().to_path_buf();
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

    /// Writes all bytes and commits the destination.
    pub(crate) fn write_bytes(
        self,
        bytes: &[u8],
    ) -> Result<(), LocalAtomicWriteError> {
        self.write_with(|writer| writer.write_all(bytes))
    }

    /// Invokes caller-provided staging logic and commits the destination.
    ///
    /// The callback receives the guarded writer itself rather than a [`File`].
    /// Exposing a file would let the callback retain a cloned handle and mutate
    /// the committed inode after rename, invalidating the atomic snapshot.
    ///
    /// # Parameters
    /// - `write`: Callback that writes the complete staged contents.
    ///
    /// # Errors
    /// Returns a structured error when the callback or commit sequence fails.
    /// Callback errors are reported at
    /// [`LocalAtomicWriteStage::WriteTemporaryFile`].
    ///
    /// # Panics
    /// Propagates callback panics after the staging guard attempts best-effort
    /// cleanup while unwinding.
    pub(crate) fn write_with<F>(
        mut self,
        write: F,
    ) -> Result<(), LocalAtomicWriteError>
    where
        F: FnOnce(&mut Self) -> io::Result<()>,
    {
        let result = write(&mut self);
        with_staging_cleanup(
            result,
            LocalAtomicWriteStage::WriteTemporaryFile,
            &self.path,
            &mut self.staged_file,
        )?;
        self.commit()
    }
}

impl Write for LocalAtomicWriter {
    /// Writes bytes into the private staging file.
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.staged_file.file_mut().write(buffer)
    }

    /// Writes bytes from multiple buffers into the private staging file.
    #[inline(always)]
    fn write_vectored(
        &mut self,
        buffers: &[io::IoSlice<'_>],
    ) -> io::Result<usize> {
        self.staged_file.file_mut().write_vectored(buffers)
    }

    /// Flushes userspace data into the private staging file.
    #[inline(always)]
    fn flush(&mut self) -> io::Result<()> {
        self.staged_file.file_mut().flush()
    }
}

/// Adds atomic-write context to a native I/O result.
fn with_atomic_context<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    temporary_path: Option<PathBuf>,
    destination_state: LocalAtomicDestinationState,
) -> Result<T, LocalAtomicWriteError> {
    result.map_err(|source| {
        LocalAtomicWriteError::new(
            stage,
            path.to_path_buf(),
            temporary_path,
            destination_state,
            source,
        )
    })
}

/// Creates an atomic-write error and explicitly cleans up its staging file.
fn atomic_error_with_staging(
    stage: LocalAtomicWriteStage,
    path: &Path,
    source: io::Error,
    staged_file: &mut StagedFile,
) -> LocalAtomicWriteError {
    atomic_error_with_staging_state(
        stage,
        path,
        source,
        LocalAtomicDestinationState::Unchanged,
        staged_file,
    )
}

/// Creates an atomic error and handles staging according to destination state.
fn atomic_error_with_staging_state(
    stage: LocalAtomicWriteStage,
    path: &Path,
    source: io::Error,
    destination_state: LocalAtomicDestinationState,
    staged_file: &mut StagedFile,
) -> LocalAtomicWriteError {
    let temporary_path = staged_file.path().to_path_buf();
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

/// Adds atomic-write context and cleanup to a staging operation result.
fn with_staging_cleanup<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    staged_file: &mut StagedFile,
) -> Result<T, LocalAtomicWriteError> {
    result.map_err(|source| {
        atomic_error_with_staging(stage, path, source, staged_file)
    })
}

/// Synchronizes the destination and every newly created parent entry.
fn sync_atomic_parent_chain(
    path: &Path,
    parent_dirs_to_sync: &[PathBuf],
) -> io::Result<()> {
    sync_parent_dir(path)?;
    for directory in parent_dirs_to_sync.iter().rev() {
        sync_parent_dir(directory)?;
    }
    Ok(())
}

/// Returns existing regular-file metadata for atomic validation.
fn existing_file_metadata(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(Some(metadata)),
        Ok(_) => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "atomic write destination must be absent or a regular file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(add_path_context(error, "read destination metadata", path))
        }
    }
}
