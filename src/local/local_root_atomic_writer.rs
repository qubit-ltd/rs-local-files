// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Streaming descriptor-relative atomic file replacement.
// qubit-style: allow source-test-pair

#[cfg(unix)]
use std::ffi::CString;
#[cfg(any(unix, windows))]
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::time::Duration;

#[cfg(windows)]
use super::create_rooted_directory;
#[cfg(unix)]
use super::internal::AtomicInstallRecovery;
#[cfg(any(unix, windows))]
use super::internal::LocalAtomicPublicationMode;
#[cfg(unix)]
use super::internal::OpenedAtomicDestination;
#[cfg(unix)]
use super::internal::RootedParentMode;
#[cfg(unix)]
use super::internal::RootedStagedFile;
#[cfg(windows)]
use super::internal::WindowsRootedStagedFile;
use super::internal::commit_recoverably;
#[cfg(unix)]
use super::internal::create_rooted_staged_file;
use super::internal::finalize_failed_commit;
#[cfg(unix)]
use super::internal::inspect_rooted_atomic_destination;
#[cfg(unix)]
use super::internal::install_rooted_atomic_file;
#[cfg(unix)]
use super::internal::open_rooted_atomic_destination;
#[cfg(unix)]
use super::internal::open_rooted_parent;
#[cfg(unix)]
use super::internal::preserve_atomic_metadata;
#[cfg(unix)]
use super::internal::recover_atomic_install_error;
use super::internal::synchronize_staging_file;
#[cfg(all(feature = "internal-test-support", unix))]
use super::internal::test_support;
#[cfg(unix)]
use super::internal::verify_rooted_atomic_destination_identity;
#[cfg(windows)]
use super::open_rooted_native_writer;
#[cfg(windows)]
use super::read_rooted_symlink_metadata;
#[cfg(windows)]
use super::rename_rooted_entry;
#[cfg(windows)]
use super::try_random_file_name;
use crate::LocalAtomicCommitError;
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
#[cfg(any(unix, windows))]
use crate::LocalAtomicWriteOptions;
use crate::LocalAtomicWriteStage;
use crate::LocalDurabilityRequirement;
#[cfg(any(unix, windows))]
use crate::LocalRelativePath;
#[cfg(windows)]
use crate::write::Mode as WriteMode;
#[cfg(windows)]
use crate::write::OpenOptions as WriteOpenOptions;

/// A streaming atomic writer contained by an open [`crate::rooted::Root`].
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
#[must_use = "rooted atomic writes have no effect unless committed"]
#[derive(Debug)]
pub(crate) struct LocalRootAtomicWriter {
    /// Requested relative destination retained for structured errors.
    path: PathBuf,
    /// Optional limit for retrying a nonblocking destination open.
    #[cfg(unix)]
    open_retry_timeout: Option<Duration>,
    /// Final destination entry name within the staging parent.
    #[cfg(unix)]
    final_name: CString,
    /// Parents whose newly created child entries require synchronization.
    #[cfg(unix)]
    parent_dirs_to_sync: Vec<File>,
    /// Whether a regular destination existed when this writer began.
    #[cfg(unix)]
    destination_existed: bool,
    /// Whether existing regular-file metadata must be preserved.
    #[cfg(unix)]
    preserve_destination_metadata: bool,
    /// Durability requested for staging and parent synchronization.
    #[cfg(unix)]
    durability: LocalDurabilityRequirement,
    /// Descriptor-relative staging lifecycle.
    #[cfg(unix)]
    staged_file: RootedStagedFile,
    /// Final rooted destination.
    #[cfg(windows)]
    destination: LocalRelativePath,
    /// Whether a regular destination existed when this writer began.
    #[cfg(windows)]
    destination_existed: bool,
    /// Whether existing regular-file metadata must be preserved.
    #[cfg(windows)]
    preserve_destination_metadata: bool,
    /// Durability requested for staging and parent synchronization.
    #[cfg(windows)]
    durability: LocalDurabilityRequirement,
    /// Handle-relative staging lifecycle.
    #[cfg(windows)]
    staged_file: WindowsRootedStagedFile,
}

impl LocalRootAtomicWriter {
    /// Creates a rooted atomic writer from an open root capability.
    ///
    /// # Parameters
    ///
    /// * `root` - Open root directory authority.
    /// * `diagnostic_root` - Path used only to contextualize traversal errors.
    /// * `path` - Validated relative destination.
    /// * `options` - Parent-creation and destination-open retry policy.
    ///
    /// # Returns
    ///
    /// An armed rooted atomic writer.
    ///
    /// # Errors
    ///
    /// Returns a structured error for parent preparation, destination
    /// inspection, or staging-file creation failures.
    #[cfg(unix)]
    pub(crate) fn new(
        root: &File,
        diagnostic_root: &Path,
        path: &LocalRelativePath,
        options: LocalAtomicWriteOptions,
    ) -> Result<Self, LocalAtomicWriteError> {
        let requested_path = path.as_path().to_path_buf();
        let diagnostic_path = diagnostic_root.join(path.as_path());
        let parent_mode = if options.creates_parent() {
            RootedParentMode::CreateMissingAndTrackSync
        } else {
            RootedParentMode::OpenExisting
        };
        let rooted_parent = map_atomic_error(
            open_rooted_parent(root, &diagnostic_path, path, parent_mode),
            LocalAtomicWriteStage::PrepareParent,
            &requested_path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        let (parent, final_name, parent_dirs_to_sync) = rooted_parent.into_parts();
        let (destination_existed, preserve_destination_metadata) = map_atomic_error(
            inspect_rooted_atomic_destination(
                &parent,
                &final_name,
                options.replaces_target_symlink()
                    || options.publication_mode() == LocalAtomicPublicationMode::CreateNew,
            ),
            LocalAtomicWriteStage::InspectDestination,
            &requested_path,
            None,
            LocalAtomicDestinationState::Unchanged,
        )?;
        if options.publication_mode() == LocalAtomicPublicationMode::CreateNew && destination_existed {
            return Err(LocalAtomicWriteError::new(
                LocalAtomicWriteStage::InspectDestination,
                requested_path,
                None,
                LocalAtomicDestinationState::Unchanged,
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "rooted atomic create-new destination already exists",
                ),
            ));
        }
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
            open_retry_timeout: options.open_retry_timeout(),
            final_name,
            parent_dirs_to_sync,
            destination_existed,
            preserve_destination_metadata,
            durability: options.durability(),
            staged_file,
        })
    }

    /// Creates a Windows rooted atomic writer from an open root capability.
    #[cfg(windows)]
    pub(crate) fn new(
        root: &File,
        diagnostic_root: &Path,
        path: &LocalRelativePath,
        options: LocalAtomicWriteOptions,
    ) -> Result<Self, LocalAtomicWriteError> {
        let requested_path = path.as_path().to_path_buf();
        let diagnostic_path = diagnostic_root.join(path.as_path());
        if options.creates_parent()
            && let Some(parent) = path.as_path().parent().filter(|parent| !parent.as_os_str().is_empty())
        {
            let parent = LocalRelativePath::new(parent).map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::PrepareParent,
                    requested_path.clone(),
                    None,
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
            map_atomic_error(
                create_rooted_directory(root, diagnostic_root, &parent, true, true),
                LocalAtomicWriteStage::PrepareParent,
                &requested_path,
                None,
                LocalAtomicDestinationState::Unchanged,
            )?;
        }
        let (destination_existed, preserve_destination_metadata) =
            match read_rooted_symlink_metadata(root, diagnostic_root, path) {
                Ok(file) => {
                    if options.publication_mode() == LocalAtomicPublicationMode::CreateNew {
                        return Err(LocalAtomicWriteError::new(
                            LocalAtomicWriteStage::InspectDestination,
                            requested_path,
                            None,
                            LocalAtomicDestinationState::Unchanged,
                            io::Error::new(
                                io::ErrorKind::AlreadyExists,
                                "rooted atomic create-new destination already exists",
                            ),
                        ));
                    }
                    let metadata = file.metadata().map_err(|source| {
                        LocalAtomicWriteError::new(
                            LocalAtomicWriteStage::InspectDestination,
                            requested_path.clone(),
                            None,
                            LocalAtomicDestinationState::Unchanged,
                            source,
                        )
                    })?;
                    if metadata.is_file() {
                        (true, true)
                    } else if options.replaces_target_symlink() && metadata.file_type().is_symlink() {
                        (true, false)
                    } else {
                        return Err(LocalAtomicWriteError::new(
                            LocalAtomicWriteStage::InspectDestination,
                            requested_path,
                            None,
                            LocalAtomicDestinationState::Unchanged,
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "rooted atomic destination is not a regular file",
                            ),
                        ));
                    }
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => (false, false),
                Err(source) => {
                    return Err(LocalAtomicWriteError::new(
                        LocalAtomicWriteStage::InspectDestination,
                        requested_path,
                        None,
                        LocalAtomicDestinationState::Unchanged,
                        source,
                    ));
                }
            };
        let relative_parent = path.as_path().parent().unwrap_or(Path::new(""));
        let staging_root = root.try_clone().map_err(|source| {
            LocalAtomicWriteError::new(
                LocalAtomicWriteStage::CreateTemporaryFile,
                requested_path.clone(),
                None,
                LocalAtomicDestinationState::Unchanged,
                source,
            )
        })?;
        let mut last_collision = None;
        for _ in 0..32 {
            let name = try_random_file_name(".qubit-atomic-", None, Some(".tmp")).map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::CreateTemporaryFile,
                    requested_path.clone(),
                    None,
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
            let staging_path = if relative_parent.as_os_str().is_empty() {
                LocalRelativePath::new(&name)
            } else {
                LocalRelativePath::new(relative_parent).and_then(|parent| parent.join(&name))
            }
            .map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::CreateTemporaryFile,
                    requested_path.clone(),
                    None,
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
            let write_options = WriteOpenOptions::new(WriteMode::CreateNew);
            match open_rooted_native_writer(root, diagnostic_root, &staging_path, &write_options) {
                Ok(file) => {
                    return Ok(Self {
                        path: requested_path,
                        destination: path.clone(),
                        destination_existed,
                        staged_file: WindowsRootedStagedFile {
                            root: staging_root,
                            path: staging_path.clone(),
                            diagnostic_path: diagnostic_root.join(staging_path.as_path()),
                            file: Some(file),
                            armed: true,
                        },
                        preserve_destination_metadata,
                        durability: options.durability(),
                    });
                }
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                    last_collision = Some(source);
                }
                Err(source) => {
                    return Err(LocalAtomicWriteError::new(
                        LocalAtomicWriteStage::CreateTemporaryFile,
                        requested_path,
                        Some(diagnostic_path),
                        LocalAtomicDestinationState::Unchanged,
                        source,
                    ));
                }
            }
        }
        Err(LocalAtomicWriteError::new(
            LocalAtomicWriteStage::CreateTemporaryFile,
            requested_path,
            Some(diagnostic_path),
            LocalAtomicDestinationState::Unchanged,
            last_collision.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "could not allocate a unique rooted staging file",
                )
            }),
        ))
    }

    /// Consumes the writer and reports whether requested durability completed.
    #[inline]
    pub(crate) fn commit_with_durability(self) -> Result<bool, LocalAtomicWriteError> {
        self.commit_recoverable_with_durability()
            .map_err(|error| error.into_final_error_with(Self::finalize_failed_commit))
    }

    /// Attempts commit and reports whether requested durability completed.
    ///
    /// # Returns
    ///
    /// `true` only when both staging data and the parent namespace were
    /// synchronized. Preferred durability may publish with `false`.
    ///
    /// # Errors
    ///
    /// Returns a recoverable error before installation or a terminal error
    /// after destination state may have changed.
    pub(crate) fn commit_recoverable_with_durability(self) -> Result<bool, LocalAtomicCommitError<Self>> {
        #[cfg(unix)]
        {
            commit_recoverably(self, Self::commit_attempt, |writer| writer.staged_file.is_open())
        }
        #[cfg(windows)]
        {
            if self.durability == LocalDurabilityRequirement::Required {
                return Err(LocalAtomicCommitError::new(
                    unsupported_atomic_error(&self.path),
                    Some(self),
                ));
            }
            commit_recoverably(self, Self::commit_attempt_windows, |writer| writer.staged_file.armed)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let error = unsupported_atomic_error(&self.path);
            Err(LocalAtomicCommitError::new(error, None))
        }
    }

    /// Aborts replacement and removes the descriptor-relative staging entry.
    ///
    /// # Errors
    ///
    /// Returns a structured cleanup error when `unlinkat` fails.
    #[cfg_attr(not(any(unix, windows)), allow(unused_mut))]
    pub(crate) fn abort(&mut self) -> Result<(), LocalAtomicWriteError> {
        #[cfg(unix)]
        {
            let temporary_path = self.staged_file.diagnostic_path().to_path_buf();
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
        #[cfg(windows)]
        {
            let temporary_path = self.staged_file.diagnostic_path.clone();
            self.staged_file.cleanup().map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::CleanupTemporaryFile,
                    self.path.clone(),
                    Some(temporary_path),
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(unsupported_atomic_error(&self.path))
        }
    }

    /// Runs one handle-relative Windows commit attempt.
    #[cfg(windows)]
    fn commit_attempt_windows(&mut self) -> Result<bool, LocalAtomicWriteError> {
        let destination = if self.preserve_destination_metadata {
            Some(
                read_rooted_symlink_metadata(&self.staged_file.root, Path::new(""), &self.destination).map_err(
                    |source| {
                        LocalAtomicWriteError::new(
                            LocalAtomicWriteStage::ReadDestinationMetadata,
                            self.path.clone(),
                            Some(self.staged_file.diagnostic_path.clone()),
                            LocalAtomicDestinationState::Unchanged,
                            source,
                        )
                    },
                )?,
            )
        } else {
            None
        };
        if let Some(destination) = destination.as_ref() {
            let permissions = destination
                .metadata()
                .map_err(|source| {
                    LocalAtomicWriteError::new(
                        LocalAtomicWriteStage::ReadDestinationMetadata,
                        self.path.clone(),
                        Some(self.staged_file.diagnostic_path.clone()),
                        LocalAtomicDestinationState::Unchanged,
                        source,
                    )
                })?
                .permissions();
            self.staged_file.file().set_permissions(permissions).map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::ApplyDestinationMetadata,
                    self.path.clone(),
                    Some(self.staged_file.diagnostic_path.clone()),
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
        }
        if self.durability == LocalDurabilityRequirement::Required {
            self.staged_file.file().sync_all().map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::SyncTemporaryFile,
                    self.path.clone(),
                    Some(self.staged_file.diagnostic_path.clone()),
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
        } else if self.durability == LocalDurabilityRequirement::Preferred {
            let _ = self.staged_file.file().sync_all();
        }
        if let Some(opened_destination) = destination.as_ref() {
            let current_destination =
                read_rooted_symlink_metadata(&self.staged_file.root, Path::new(""), &self.destination).map_err(
                    |source| {
                        LocalAtomicWriteError::new(
                            LocalAtomicWriteStage::ReadDestinationMetadata,
                            self.path.clone(),
                            Some(self.staged_file.diagnostic_path.clone()),
                            LocalAtomicDestinationState::Unchanged,
                            source,
                        )
                    },
                )?;
            let opened = crate::rooted::Metadata::from_open_file(opened_destination).map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::ReadDestinationMetadata,
                    self.path.clone(),
                    Some(self.staged_file.diagnostic_path.clone()),
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
            let current = crate::rooted::Metadata::from_open_file(&current_destination).map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::ReadDestinationMetadata,
                    self.path.clone(),
                    Some(self.staged_file.diagnostic_path.clone()),
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
            if !opened.is_same_file(&current) {
                return Err(LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::ReplaceDestination,
                    self.path.clone(),
                    Some(self.staged_file.diagnostic_path.clone()),
                    LocalAtomicDestinationState::Indeterminate,
                    io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "rooted atomic destination changed during commit",
                    ),
                ));
            }
        }
        rename_rooted_entry(
            &self.staged_file.root,
            Path::new(""),
            &self.staged_file.path,
            &self.destination,
            self.destination_existed,
        )
        .map_err(|source| {
            LocalAtomicWriteError::new(
                LocalAtomicWriteStage::ReplaceDestination,
                self.path.clone(),
                Some(self.staged_file.diagnostic_path.clone()),
                LocalAtomicDestinationState::Unchanged,
                source,
            )
        })?;
        self.staged_file.disarm();
        Ok(false)
    }

    /// Runs one rooted commit attempt without consuming recoverable staging.
    ///
    /// # Errors
    ///
    /// Returns the structured commit failure. Errors raised before installation
    /// leave the staging handle open for the public recoverable commit API.
    #[cfg(unix)]
    fn commit_attempt(&mut self) -> Result<bool, LocalAtomicWriteError> {
        let destination = self.open_destination_for_commit()?;
        self.preserve_destination_metadata(destination.as_ref())?;
        let file_durable = self.sync_temporary_file()?;
        self.verify_destination_for_commit(destination.as_ref())?;
        let parent_durable = self.install_and_sync_parent()?;
        Ok(file_durable && parent_durable)
    }

    /// Opens the existing rooted destination for commit-time metadata.
    ///
    /// # Returns
    ///
    /// The descriptor-relative destination when one existed at writer
    /// creation, or `None` when commit will install a new entry.
    ///
    /// # Errors
    ///
    /// Returns a structured metadata-stage error when the destination cannot
    /// be opened or disappeared before commit. The staging writer remains
    /// available for retry or explicit abort.
    #[cfg(unix)]
    fn open_destination_for_commit(&mut self) -> Result<Option<OpenedAtomicDestination>, LocalAtomicWriteError> {
        if !self.preserve_destination_metadata {
            return Ok(None);
        }
        let destination_result =
            open_rooted_atomic_destination(self.staged_file.parent(), &self.final_name, self.open_retry_timeout);
        let opened = map_atomic_error(
            destination_result,
            LocalAtomicWriteStage::ReadDestinationMetadata,
            &self.path,
            Some(self.staged_file.diagnostic_path().to_path_buf()),
            LocalAtomicDestinationState::Unchanged,
        )?;
        match opened {
            Some(destination) => Ok(Some(destination)),
            None => Err(LocalAtomicWriteError::new(
                LocalAtomicWriteStage::ReadDestinationMetadata,
                self.path.clone(),
                Some(self.staged_file.diagnostic_path().to_path_buf()),
                LocalAtomicDestinationState::Missing,
                io::Error::new(io::ErrorKind::NotFound, "rooted atomic destination disappeared"),
            )),
        }
    }

    /// Copies strict metadata from a rooted destination to staging.
    ///
    /// # Parameters
    ///
    /// * `destination` - Opened destination, or `None` for a new entry.
    ///
    /// # Errors
    ///
    /// Returns a structured metadata-application error while retaining staging
    /// when platform metadata cannot be preserved.
    #[cfg(unix)]
    fn preserve_destination_metadata(
        &mut self,
        destination: Option<&OpenedAtomicDestination>,
    ) -> Result<(), LocalAtomicWriteError> {
        let Some(destination) = destination else {
            return Ok(());
        };
        let result = preserve_atomic_metadata(destination.file(), self.staged_file.file());
        map_atomic_error(
            result,
            LocalAtomicWriteStage::ApplyDestinationMetadata,
            &self.path,
            Some(self.staged_file.diagnostic_path().to_path_buf()),
            LocalAtomicDestinationState::Unchanged,
        )
    }

    /// Synchronizes the rooted staging file before installation.
    ///
    /// # Errors
    ///
    /// Returns a structured staging synchronization error while retaining
    /// staging when the native synchronization fails.
    #[cfg(unix)]
    fn sync_temporary_file(&mut self) -> Result<bool, LocalAtomicWriteError> {
        synchronize_staging_file(self.staged_file.file(), self.durability, |result| {
            map_atomic_error(
                result,
                LocalAtomicWriteStage::SyncTemporaryFile,
                &self.path,
                Some(self.staged_file.diagnostic_path().to_path_buf()),
                LocalAtomicDestinationState::Unchanged,
            )
        })
    }

    /// Verifies that the rooted destination still names the opened file.
    ///
    /// # Parameters
    ///
    /// * `destination` - Opened destination, or `None` for a new entry.
    ///
    /// # Errors
    ///
    /// Returns the structured namespace-race error produced by the rooted
    /// identity verifier.
    #[cfg(unix)]
    #[inline]
    fn verify_destination_for_commit(
        &mut self,
        destination: Option<&OpenedAtomicDestination>,
    ) -> Result<(), LocalAtomicWriteError> {
        let Some(destination) = destination else {
            return Ok(());
        };
        verify_rooted_atomic_destination_identity(&self.final_name, destination, &self.path, &self.staged_file)
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
    #[cfg(unix)]
    #[inline]
    fn finalize_failed_commit(self, error: LocalAtomicWriteError) -> LocalAtomicWriteError {
        finalize_failed_commit(
            self,
            error,
            |writer| writer.staged_file.cleanup(),
            |writer| {
                writer.staged_file.close();
                writer.staged_file.disarm();
            },
        )
    }

    /// Finalizes a consuming Windows commit failure.
    #[cfg(windows)]
    #[inline]
    fn finalize_failed_commit(mut self, error: LocalAtomicWriteError) -> LocalAtomicWriteError {
        finalize_failed_commit(self, error, |writer| writer.staged_file.cleanup(), |_| {})
    }

    /// Returns an unsupported failure after a non-Unix commit attempt.
    ///
    /// # Parameters
    ///
    /// * `error` - Unsupported rooted atomic-write failure.
    ///
    /// # Returns
    ///
    /// The unchanged unsupported failure.
    #[cfg(not(any(unix, windows)))]
    #[inline(always)]
    fn finalize_failed_commit(self, error: LocalAtomicWriteError) -> LocalAtomicWriteError {
        error
    }

    /// Installs rooted staging and synchronizes the parent descriptor chain.
    ///
    /// # Errors
    ///
    /// Returns the structured installation or recovery error, or a parent
    /// synchronization error after the destination has been replaced.
    #[cfg(unix)]
    fn install_and_sync_parent(&mut self) -> Result<bool, LocalAtomicWriteError> {
        let install_result =
            install_rooted_atomic_file(&mut self.staged_file, &self.final_name, self.destination_existed);
        if let Err((source, destination_state, staging_state)) = install_result {
            return recover_atomic_install_error(
                AtomicInstallRecovery {
                    path: &self.path,
                    temporary_path: self.staged_file.diagnostic_path().to_path_buf(),
                    source,
                    destination_state,
                    staging_state,
                },
                &mut self.staged_file,
                RootedStagedFile::cleanup,
                |staged_file: &mut RootedStagedFile| {
                    staged_file.close();
                    staged_file.disarm();
                },
                |staged_file: &RootedStagedFile| {
                    sync_rooted_parent_chain(staged_file.parent(), &self.parent_dirs_to_sync)
                },
            )
            .map(|()| false);
        }
        if self.durability == LocalDurabilityRequirement::NotRequired {
            self.staged_file.disarm();
            return Ok(false);
        }
        let temporary_path = self.staged_file.diagnostic_path().to_path_buf();
        self.staged_file.disarm();
        match sync_rooted_parent_chain(self.staged_file.parent(), &self.parent_dirs_to_sync) {
            Ok(()) => Ok(true),
            Err(_) if self.durability == LocalDurabilityRequirement::Preferred => Ok(false),
            Err(error) => map_atomic_error(
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

impl Write for LocalRootAtomicWriter {
    /// Writes bytes into the private rooted staging file.
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            self.staged_file.file_mut().write(buffer)
        }
        #[cfg(windows)]
        {
            self.staged_file.file_mut().write(buffer)
        }
        #[cfg(not(any(unix, windows)))]
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
    fn write_vectored(&mut self, buffers: &[io::IoSlice<'_>]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            self.staged_file.file_mut().write_vectored(buffers)
        }
        #[cfg(windows)]
        {
            self.staged_file.file_mut().write_vectored(buffers)
        }
        #[cfg(not(any(unix, windows)))]
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
        #[cfg(windows)]
        {
            self.staged_file.file_mut().flush()
        }
        #[cfg(not(any(unix, windows)))]
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
fn sync_rooted_parent_chain(parent: &File, parent_dirs_to_sync: &[File]) -> io::Result<()> {
    #[cfg(feature = "internal-test-support")]
    if test_support::is_enabled("atomic-install-unlink-recover-sync")
        || test_support::is_enabled("atomic-install-unlink-persistent-sync")
        || test_support::is_enabled("atomic-install-unlink-indeterminate-sync")
        || test_support::is_enabled("rooted-preferred-parent-sync")
    {
        return Err(crate::local::test_fault_error());
    }
    parent.sync_all()?;
    for directory in parent_dirs_to_sync.iter().rev() {
        directory.sync_all()?;
    }
    Ok(())
}

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
#[cfg(any(unix, windows))]
#[inline]
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

/// Creates a structured unsupported rooted atomic-write error.
///
/// # Parameters
///
/// * `path` - Requested relative destination.
///
/// # Returns
///
/// An unsupported error that never falls back to ordinary path authority.
#[cfg(not(unix))]
#[inline]
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
