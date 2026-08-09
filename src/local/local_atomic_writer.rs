// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Streaming durable atomic file replacement.
// qubit-style: allow source-test-pair

use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::time::Duration;

use super::internal::AtomicInstallRecovery;
use super::internal::DEFAULT_TEMP_ENTRY_RETRIES;
use super::internal::LocalAtomicPublicationMode;
#[cfg(unix)]
use super::internal::OpenedAtomicDestination;
use super::internal::StagedFile;
use super::internal::absolute_path;
use super::internal::add_path_context;
use super::internal::create_temp_file_in_dir;
use super::internal::ensure_parent_path_with_sync_dirs;
use super::internal::install_atomic_file;
#[cfg(unix)]
use super::internal::open_atomic_destination;
use super::internal::parent_dir_for;
#[cfg(unix)]
use super::internal::preserve_atomic_metadata;
use super::internal::recover_atomic_install_error;
use super::internal::sync_parent_dir;
#[cfg(feature = "internal-test-support")]
use super::internal::test_support;
#[cfg(unix)]
use super::internal::verify_atomic_destination_identity;
use crate::LocalAtomicCommitError;
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
use crate::LocalAtomicWriteOptions;
use crate::LocalAtomicWriteStage;
use crate::LocalDurabilityRequirement;

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
#[must_use = "atomic writes have no effect unless the writer is committed"]
#[derive(Debug)]
pub(crate) struct LocalAtomicWriter {
    /// Requested destination path.
    path: PathBuf,
    /// Absolute destination path used by filesystem operations.
    operation_path: PathBuf,
    /// Newly created parent directories that require synchronization.
    parent_dirs_to_sync: Vec<PathBuf>,
    /// Whether a destination entry existed when this writer began.
    destination_existed: bool,
    /// Whether commit preserves metadata from an existing regular file.
    preserve_destination_metadata: bool,
    /// Durability requested for this publication.
    durability: LocalDurabilityRequirement,
    #[cfg(unix)]
    /// Optional limit for retrying a nonblocking destination open.
    open_retry_timeout: Option<Duration>,
    /// Owned same-directory staging file.
    staged_file: StagedFile,
}

#[allow(dead_code)]
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
        let (destination_existed, preserve_destination_metadata) = if options
            .publication_mode()
            == LocalAtomicPublicationMode::CreateNew
        {
            match fs::symlink_metadata(&operation_path) {
                Ok(_) => {
                    return Err(LocalAtomicWriteError::new(
                        LocalAtomicWriteStage::InspectDestination,
                        path.to_path_buf(),
                        None,
                        LocalAtomicDestinationState::Unchanged,
                        io::Error::new(
                            ErrorKind::AlreadyExists,
                            "atomic create-new destination already exists",
                        ),
                    ));
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    (false, false)
                }
                Err(error) => {
                    return Err(LocalAtomicWriteError::new(
                        LocalAtomicWriteStage::InspectDestination,
                        path.to_path_buf(),
                        None,
                        LocalAtomicDestinationState::Unchanged,
                        error,
                    ));
                }
            }
        } else {
            with_atomic_context(
                existing_file_metadata(
                    &operation_path,
                    options.replaces_target_symlink(),
                ),
                LocalAtomicWriteStage::InspectDestination,
                path,
                None,
                LocalAtomicDestinationState::Unchanged,
            )?
        };
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
            preserve_destination_metadata,
            durability: options.durability(),
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
    pub(crate) fn commit(self) -> Result<(), LocalAtomicWriteError> {
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
    #[inline]
    pub(crate) fn commit_recoverable(
        self,
    ) -> Result<(), LocalAtomicCommitError<Self>> {
        self.commit_recoverable_with_durability().map(|_| ())
    }

    /// Attempts to commit and reports whether requested durability completed.
    ///
    /// # Returns
    ///
    /// `true` only when both staging data and the destination namespace were
    /// synchronized. Preferred durability may return `false` after publication.
    ///
    /// # Errors
    ///
    /// Returns a recoverable error when publication did not begin, or a
    /// terminal error after destination state may have changed.
    pub(crate) fn commit_recoverable_with_durability(
        mut self,
    ) -> Result<bool, LocalAtomicCommitError<Self>> {
        match self.commit_attempt() {
            Ok(durable) => Ok(durable),
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
    pub(crate) fn abort(&mut self) -> Result<(), LocalAtomicWriteError> {
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
    #[inline]
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
    fn commit_attempt(&mut self) -> Result<bool, LocalAtomicWriteError> {
        #[cfg(unix)]
        let destination = self.open_destination_for_commit()?;
        #[cfg(unix)]
        self.preserve_destination_metadata(destination.as_ref())?;
        #[cfg(not(any(unix, windows)))]
        self.reject_unsupported_metadata_preservation()?;
        let file_durable = self.sync_temporary_file()?;
        #[cfg(unix)]
        self.verify_destination_for_commit(destination.as_ref())?;
        #[cfg(not(unix))]
        self.verify_non_unix_destination_for_commit()?;
        let parent_durable = self.install_and_sync_parent()?;
        Ok(file_durable && parent_durable)
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
        if !self.preserve_destination_metadata {
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
        if !self.preserve_destination_metadata {
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
    fn sync_temporary_file(&mut self) -> Result<bool, LocalAtomicWriteError> {
        match self.durability {
            LocalDurabilityRequirement::NotRequired => Ok(false),
            LocalDurabilityRequirement::Preferred => {
                Ok(self.staged_file.file().sync_all().is_ok())
            }
            LocalDurabilityRequirement::Required => {
                with_atomic_context(
                    self.staged_file.file().sync_all(),
                    LocalAtomicWriteStage::SyncTemporaryFile,
                    &self.path,
                    Some(self.staged_file.path().to_path_buf()),
                    LocalAtomicDestinationState::Unchanged,
                )?;
                Ok(true)
            }
        }
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
    #[inline]
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
        let exists = if self.preserve_destination_metadata {
            let (exists, _metadata_preservation_required) =
                with_atomic_context(
                    existing_file_metadata(&self.operation_path, false),
                    LocalAtomicWriteStage::ReplaceDestination,
                    &self.path,
                    Some(self.staged_file.path().to_path_buf()),
                    LocalAtomicDestinationState::Unchanged,
                )?;
            exists
        } else {
            with_atomic_context(
                fs::symlink_metadata(&self.operation_path).map(|_| true),
                LocalAtomicWriteStage::ReplaceDestination,
                &self.path,
                Some(self.staged_file.path().to_path_buf()),
                LocalAtomicDestinationState::Unchanged,
            )?
        };
        if !exists {
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
    #[inline]
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
    fn install_and_sync_parent(
        &mut self,
    ) -> Result<bool, LocalAtomicWriteError> {
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
            )
            .map(|()| false);
        }
        if self.durability == LocalDurabilityRequirement::NotRequired {
            self.staged_file.disarm();
            return Ok(false);
        }
        let temporary_path = self.staged_file.path().to_path_buf();
        self.staged_file.disarm();
        match sync_atomic_parent_chain(
            &self.operation_path,
            &self.parent_dirs_to_sync,
        ) {
            Ok(()) => Ok(true),
            Err(_)
                if self.durability == LocalDurabilityRequirement::Preferred =>
            {
                Ok(false)
            }
            Err(error) => with_atomic_context(
                Err(error),
                LocalAtomicWriteStage::SyncParent,
                &self.path,
                Some(temporary_path),
                LocalAtomicDestinationState::Replaced,
            )
            .map(|()| true),
        }
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
#[inline]
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
#[inline]
#[allow(dead_code)]
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
#[inline(always)]
#[allow(dead_code)]
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
    #[cfg(feature = "internal-test-support")]
    if test_support::is_enabled("atomic-install-unlink-recover-sync")
        || test_support::is_enabled("atomic-install-unlink-persistent-sync")
        || test_support::is_enabled("atomic-install-unlink-indeterminate-sync")
    {
        return Err(crate::local::test_fault_error());
    }
    sync_parent_dir(path)?;
    for directory in parent_dirs_to_sync.iter().rev() {
        sync_parent_dir(directory)?;
    }
    Ok(())
}

/// Returns destination existence and metadata-preservation requirements.
fn existing_file_metadata(
    path: &Path,
    replace_target_symlink: bool,
) -> io::Result<(bool, bool)> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok((true, true)),
        Ok(metadata)
            if replace_target_symlink && metadata.file_type().is_symlink() =>
        {
            Ok((true, false))
        }
        Ok(_) => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "atomic write destination must be absent or a regular file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok((false, false)),
        Err(error) => {
            Err(add_path_context(error, "read destination metadata", path))
        }
    }
}
