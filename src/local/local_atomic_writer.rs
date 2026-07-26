// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Streaming durable atomic file replacement.
// qubit-style: allow coverage-cfg

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
#[cfg(unix)]
use std::time::Duration;

use crate::{
    LocalAtomicCommitError,
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteOptions,
    LocalAtomicWriteStage,
};

#[cfg(coverage)]
use super::internal::coverage_fault;
use super::internal::{
    AtomicInstallRecovery,
    DEFAULT_TEMP_ENTRY_RETRIES,
    StagedFile,
    absolute_path,
    add_path_context,
    create_temp_file_in_dir,
    ensure_parent_path_with_sync_dirs,
    install_atomic_file,
    parent_dir_for,
    recover_atomic_install_error,
    sync_parent_dir,
};
#[cfg(unix)]
use super::internal::{
    OpenedAtomicDestination,
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
/// replace path entries concurrently. Use [`crate::rooted::Root`] when
/// filesystem containment must be anchored to an opened directory capability.
///
/// The guard must be committed or explicitly aborted:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::atomic;
///
/// let writer = atomic::begin(std::path::Path::new("result.bin"))?;
/// writer;
/// # Ok::<(), atomic::Error>(())
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
    #[cfg(unix)]
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
    /// - `options`: Parent-directory preparation policy.
    ///
    /// # Returns
    /// A writer owning a same-directory staging file.
    ///
    /// # Errors
    /// Returns a structured error when parent preparation, destination
    /// inspection, or staging-file creation fails.
    pub(crate) fn new(
        path: &Path,
        options: LocalAtomicWriteOptions,
    ) -> Result<Self, LocalAtomicWriteError> {
        let operation_path = with_atomic_context(
            absolute_path(path),
            LocalAtomicWriteStage::PrepareParent,
            path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        let parent_dirs_to_sync = if options.creates_parent() {
            with_atomic_context(
                ensure_parent_path_with_sync_dirs(&operation_path),
                LocalAtomicWriteStage::PrepareParent,
                path,
                None,
                LocalAtomicDestinationState::Unchanged,
            )?
        } else {
            let parent = parent_dir_for(&operation_path);
            let metadata = with_atomic_context(
                fs::metadata(parent),
                LocalAtomicWriteStage::PrepareParent,
                path,
                None,
                LocalAtomicDestinationState::Unchanged,
            )?;
            if !metadata.is_dir() {
                return Err(LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::PrepareParent,
                    path.to_path_buf(),
                    None,
                    LocalAtomicDestinationState::Unchanged,
                    io::Error::new(
                        ErrorKind::NotADirectory,
                        "atomic write parent must be a directory",
                    ),
                ));
            }
            Vec::new()
        };
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
            #[cfg(unix)]
            open_retry_timeout: options.open_retry_timeout(),
            staged_file: StagedFile::new(temp_path, file),
        })
    }

    /// Synchronizes and atomically replaces the destination.
    ///
    /// Existing Unix metadata is read from an opened destination during this
    /// call, not when the writer was created. Windows delegates strict metadata
    /// merging to `ReplaceFileW`. Immediately before replacement, commit
    /// verifies the applicable destination type and identity rules.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the replacement and required directory synchronization
    /// complete.
    ///
    /// # Errors
    ///
    /// Returns a structured error when metadata preservation, staging-file
    /// synchronization, destination replacement, or parent synchronization
    /// fails. Inspect [`LocalAtomicWriteError::destination_state`] before
    /// deciding whether the destination or retained staging path needs
    /// recovery.
    #[inline(always)]
    pub fn commit(self) -> Result<(), LocalAtomicWriteError> {
        self.commit_recoverable().map_err(|error| {
            error.into_final_error_with(Self::finalize_failed_commit)
        })
    }

    /// Attempts to commit while retaining a recoverable staging writer.
    ///
    /// Failures detected before installation begins return the writer through
    /// [`LocalAtomicCommitError::writer`] so callers can retry or explicitly
    /// abort it. Failures after installation begins are terminal and do not
    /// return a writer.
    ///
    /// # Returns
    ///
    /// `Ok(())` after a successful commit.
    ///
    /// # Errors
    ///
    /// Returns a recoverable commit error when metadata preservation,
    /// staging-file synchronization, destination replacement, or parent
    /// synchronization fails.
    pub fn commit_recoverable(
        mut self,
    ) -> Result<(), LocalAtomicCommitError<Self>> {
        match self.commit_attempt() {
            Ok(()) => Ok(()),
            Err(error) if self.staged_file.is_open() => {
                Err(LocalAtomicCommitError::new(error, Some(self)))
            }
            Err(error) => Err(LocalAtomicCommitError::new(error, None)),
        }
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
    #[inline(always)]
    pub(crate) fn write_bytes(
        self,
        bytes: &[u8],
    ) -> Result<(), LocalAtomicWriteError> {
        self.write_with(|writer| writer.write_all(bytes))
    }

    /// Invokes caller-provided staging logic and commits the destination.
    ///
    /// The callback receives the guarded writer itself rather than a
    /// [`std::fs::File`]. Exposing a file would let the callback retain a
    /// cloned handle and mutate the committed inode after rename,
    /// invalidating the atomic snapshot.
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

    /// Runs one commit attempt without consuming recoverable staging state.
    ///
    /// # Errors
    ///
    /// Returns the structured commit failure. Errors raised before installation
    /// leave the staging handle open for the public recoverable commit API.
    fn commit_attempt(&mut self) -> Result<(), LocalAtomicWriteError> {
        #[cfg(unix)]
        let destination = self.open_destination_for_commit()?;
        #[cfg(unix)]
        self.preserve_destination_metadata(destination.as_ref())?;
        #[cfg(not(any(unix, windows)))]
        self.reject_unsupported_metadata_preservation()?;
        self.sync_temporary_file()?;
        #[cfg(unix)]
        self.verify_destination_for_commit(destination.as_ref())?;
        #[cfg(not(unix))]
        self.verify_non_unix_destination_for_commit()?;
        self.install_and_sync_parent()
    }

    #[cfg(unix)]
    /// Opens the existing destination that supplies commit-time metadata.
    ///
    /// # Returns
    ///
    /// The opened destination when one existed at writer creation, or `None`
    /// when this commit will install a new destination.
    ///
    /// # Errors
    ///
    /// Returns a structured metadata-stage error when the destination cannot
    /// be opened or disappeared before commit. The staging writer remains
    /// available for retry or explicit abort.
    fn open_destination_for_commit(
        &mut self,
    ) -> Result<Option<OpenedAtomicDestination>, LocalAtomicWriteError> {
        if !self.destination_existed {
            return Ok(None);
        }
        let temporary_path = Some(self.staged_file.path().to_path_buf());
        let opened = with_atomic_context(
            open_atomic_destination(
                &self.operation_path,
                self.open_retry_timeout,
            ),
            LocalAtomicWriteStage::ReadDestinationMetadata,
            &self.path,
            temporary_path,
            LocalAtomicDestinationState::Unchanged,
        )?;
        match opened {
            Some(destination) => Ok(Some(destination)),
            None => Err(LocalAtomicWriteError::new(
                LocalAtomicWriteStage::ReadDestinationMetadata,
                self.path.clone(),
                Some(self.staged_file.path().to_path_buf()),
                LocalAtomicDestinationState::Missing,
                io::Error::new(
                    ErrorKind::NotFound,
                    "atomic write destination disappeared",
                ),
            )),
        }
    }

    #[cfg(unix)]
    /// Copies strict metadata from an opened destination to staging.
    ///
    /// # Parameters
    ///
    /// * `destination` - Opened destination, or `None` for a new file.
    ///
    /// # Errors
    ///
    /// Returns a structured metadata-application error while retaining staging
    /// when platform metadata cannot be preserved.
    fn preserve_destination_metadata(
        &mut self,
        destination: Option<&OpenedAtomicDestination>,
    ) -> Result<(), LocalAtomicWriteError> {
        let Some(destination) = destination else {
            return Ok(());
        };
        let result = preserve_atomic_metadata(
            destination.file(),
            self.staged_file.file(),
        );
        with_atomic_context(
            result,
            LocalAtomicWriteStage::ApplyDestinationMetadata,
            &self.path,
            Some(self.staged_file.path().to_path_buf()),
            LocalAtomicDestinationState::Unchanged,
        )
    }

    #[cfg(not(any(unix, windows)))]
    /// Rejects an existing destination without strict metadata support.
    ///
    /// # Errors
    ///
    /// Returns a structured unsupported metadata-application error while
    /// retaining staging when the destination already exists.
    fn reject_unsupported_metadata_preservation(
        &mut self,
    ) -> Result<(), LocalAtomicWriteError> {
        if !self.destination_existed {
            return Ok(());
        }
        with_atomic_context(
            Err(io::Error::new(
                ErrorKind::Unsupported,
                "strict atomic metadata preservation is unsupported on this target",
            )),
            LocalAtomicWriteStage::ApplyDestinationMetadata,
            &self.path,
            Some(self.staged_file.path().to_path_buf()),
            LocalAtomicDestinationState::Unchanged,
        )
    }

    /// Synchronizes staged contents and metadata before installation.
    ///
    /// # Errors
    ///
    /// Returns a structured staging synchronization error while retaining
    /// staging when the native synchronization fails.
    fn sync_temporary_file(&mut self) -> Result<(), LocalAtomicWriteError> {
        let result = self.staged_file.file().sync_all();
        with_atomic_context(
            result,
            LocalAtomicWriteStage::SyncTemporaryFile,
            &self.path,
            Some(self.staged_file.path().to_path_buf()),
            LocalAtomicDestinationState::Unchanged,
        )
    }

    #[cfg(unix)]
    /// Verifies that the opened destination still names the final entry.
    ///
    /// # Parameters
    ///
    /// * `destination` - Opened destination, or `None` for a new file.
    ///
    /// # Errors
    ///
    /// Returns the structured namespace-race error produced by the identity
    /// verifier while retaining staging for retry or explicit abort.
    fn verify_destination_for_commit(
        &mut self,
        destination: Option<&OpenedAtomicDestination>,
    ) -> Result<(), LocalAtomicWriteError> {
        let Some(destination) = destination else {
            return Ok(());
        };
        verify_atomic_destination_identity(
            &self.operation_path,
            destination,
            &self.path,
            self.staged_file.path(),
        )
    }

    #[cfg(not(unix))]
    /// Verifies that an existing non-Unix destination remains present.
    ///
    /// # Errors
    ///
    /// Returns a structured replacement-stage error when destination
    /// inspection fails or the destination disappeared before installation.
    fn verify_non_unix_destination_for_commit(
        &mut self,
    ) -> Result<(), LocalAtomicWriteError> {
        if !self.destination_existed {
            return Ok(());
        }
        let metadata = with_atomic_context(
            existing_file_metadata(&self.operation_path),
            LocalAtomicWriteStage::ReplaceDestination,
            &self.path,
            Some(self.staged_file.path().to_path_buf()),
            LocalAtomicDestinationState::Unchanged,
        )?;
        if metadata.is_none() {
            return Err(LocalAtomicWriteError::new(
                LocalAtomicWriteStage::ReplaceDestination,
                self.path.clone(),
                Some(self.staged_file.path().to_path_buf()),
                LocalAtomicDestinationState::Missing,
                io::Error::new(
                    ErrorKind::NotFound,
                    "atomic write destination disappeared",
                ),
            ));
        }
        Ok(())
    }

    /// Applies the historical cleanup policy for consuming commit failures.
    ///
    /// # Parameters
    ///
    /// * `error` - Recoverable pre-installation failure to finalize.
    ///
    /// # Returns
    ///
    /// The failure enriched with any staging cleanup error.
    fn finalize_failed_commit(
        mut self,
        error: LocalAtomicWriteError,
    ) -> LocalAtomicWriteError {
        if error.destination_state() == LocalAtomicDestinationState::Unchanged {
            error.with_cleanup_error(self.staged_file.cleanup().err())
        } else {
            self.staged_file.close();
            self.staged_file.disarm();
            error
        }
    }

    /// Installs staging and synchronizes the destination parent chain.
    ///
    /// # Errors
    ///
    /// Returns the structured installation or recovery error, or a parent
    /// synchronization error after the destination has been replaced.
    fn install_and_sync_parent(&mut self) -> Result<(), LocalAtomicWriteError> {
        self.staged_file.close();
        let install_result = install_atomic_file(
            self.staged_file.path(),
            &self.operation_path,
            self.destination_existed,
        );
        if let Err((source, destination_state, staging_state)) = install_result
        {
            return recover_atomic_install_error(
                AtomicInstallRecovery {
                    path: &self.path,
                    temporary_path: self.staged_file.path().to_path_buf(),
                    source,
                    destination_state,
                    staging_state,
                },
                &mut self.staged_file,
                StagedFile::cleanup,
                |staged_file: &mut StagedFile| {
                    staged_file.close();
                    staged_file.disarm();
                },
                |_: &StagedFile| {
                    sync_atomic_parent_chain(
                        &self.operation_path,
                        &self.parent_dirs_to_sync,
                    )
                },
            );
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
    let temporary_path = staged_file.path().to_path_buf();
    let cleanup_error = staged_file.cleanup().err();
    LocalAtomicWriteError::new(
        stage,
        path.to_path_buf(),
        Some(temporary_path),
        LocalAtomicDestinationState::Unchanged,
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
    #[cfg(coverage)]
    if coverage_fault::is_enabled("atomic-install-unlink-recover-sync")
        || coverage_fault::is_enabled("atomic-install-unlink-persistent-sync")
        || coverage_fault::is_enabled(
            "atomic-install-unlink-indeterminate-sync",
        )
    {
        return Err(io::Error::from_raw_os_error(libc::EIO));
    }
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
