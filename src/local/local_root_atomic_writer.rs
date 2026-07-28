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
#[cfg(any(unix, windows))]
use std::fs::File;
use std::io::{
    self,
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
    LocalAtomicWriteStage,
    LocalDurabilityRequirement,
};
#[cfg(any(unix, windows))]
use crate::{
    LocalAtomicWriteOptions,
    LocalRelativePath,
};

#[cfg(coverage)]
use super::internal::coverage_fault;
#[cfg(any(unix, windows))]
use super::internal::LocalAtomicPublicationMode;
#[cfg(unix)]
use super::internal::{
    AtomicInstallRecovery,
    OpenedAtomicDestination,
    RootedParentMode,
    RootedStagedFile,
    create_rooted_staged_file,
    inspect_rooted_atomic_destination,
    install_rooted_atomic_file,
    open_rooted_atomic_destination,
    open_rooted_parent,
    preserve_atomic_metadata,
    recover_atomic_install_error,
    verify_rooted_atomic_destination_identity,
};
#[cfg(windows)]
use super::{
    create_rooted_directory,
    open_rooted_native_writer,
    read_rooted_symlink_metadata,
    remove_rooted_entry,
    rename_rooted_entry,
    try_random_file_name,
};
#[cfg(windows)]
use crate::write::{
    Mode as WriteMode,
    OpenOptions as WriteOpenOptions,
};

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
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::rooted::{
///     Path,
///     Root,
/// };
///
/// let root = Root::open(".").unwrap();
/// let path = Path::new("result.bin").unwrap();
/// let writer = root.begin_atomic_write(&path).unwrap();
/// writer;
/// ```
#[must_use = "rooted atomic writes have no effect unless committed"]
#[derive(Debug)]
pub struct LocalRootAtomicWriter {
    /// Requested relative destination retained for structured errors.
    path: PathBuf,
    #[cfg(unix)]
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
    /// Whether existing regular-file metadata must be preserved.
    preserve_destination_metadata: bool,
    #[cfg(unix)]
    /// Durability requested for staging and parent synchronization.
    durability: LocalDurabilityRequirement,
    #[cfg(unix)]
    /// Descriptor-relative staging lifecycle.
    staged_file: RootedStagedFile,
    #[cfg(windows)]
    /// Final rooted destination.
    destination: LocalRelativePath,
    #[cfg(windows)]
    /// Whether a regular destination existed when this writer began.
    destination_existed: bool,
    #[cfg(windows)]
    /// Whether existing regular-file metadata must be preserved.
    preserve_destination_metadata: bool,
    #[cfg(windows)]
    /// Durability requested for staging and parent synchronization.
    durability: LocalDurabilityRequirement,
    #[cfg(windows)]
    /// Handle-relative staging lifecycle.
    staged_file: WindowsRootedStagedFile,
}

/// Owns a Windows rooted staging file and removes its name unless disarmed.
#[cfg(windows)]
#[derive(Debug)]
struct WindowsRootedStagedFile {
    /// Root capability used for cleanup and installation.
    root: File,
    /// Validated staging path beneath `root`.
    path: LocalRelativePath,
    /// Diagnostic-only absolute staging path.
    diagnostic_path: PathBuf,
    /// Open staging handle.
    file: Option<File>,
    /// Whether the staging name still requires cleanup.
    armed: bool,
}

#[cfg(windows)]
impl WindowsRootedStagedFile {
    /// Returns the open staging file.
    fn file(&self) -> &File {
        self.file
            .as_ref()
            .expect("rooted staging file must remain open while armed")
    }

    /// Returns the open staging file mutably.
    fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("rooted staging file must remain open while armed")
    }

    /// Closes and removes the staging entry.
    fn cleanup(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_ref() {
            let mut permissions = file.metadata()?.permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                file.set_permissions(permissions)?;
            }
        }
        self.file.take();
        if self.armed {
            remove_rooted_entry(&self.root, Path::new(""), &self.path, false)?;
            self.armed = false;
        }
        Ok(())
    }

    /// Marks the staging name as installed.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(windows)]
impl Drop for WindowsRootedStagedFile {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
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
        let (parent, final_name, parent_dirs_to_sync) =
            rooted_parent.into_parts();
        let (destination_existed, preserve_destination_metadata) =
            map_atomic_error(
                inspect_rooted_atomic_destination(
                    &parent,
                    &final_name,
                    options.replaces_target_symlink()
                        || options.publication_mode()
                            == LocalAtomicPublicationMode::CreateNew,
                ),
                LocalAtomicWriteStage::InspectDestination,
                &requested_path,
                None,
                LocalAtomicDestinationState::Unchanged,
            )?;
        if options.publication_mode() == LocalAtomicPublicationMode::CreateNew
            && destination_existed
        {
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

    #[cfg(windows)]
    /// Creates a Windows rooted atomic writer from an open root capability.
    pub(crate) fn new(
        root: &File,
        diagnostic_root: &Path,
        path: &LocalRelativePath,
        options: LocalAtomicWriteOptions,
    ) -> Result<Self, LocalAtomicWriteError> {
        let requested_path = path.as_path().to_path_buf();
        let diagnostic_path = diagnostic_root.join(path.as_path());
        if options.creates_parent()
            && let Some(parent) = path
                .as_path()
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
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
                create_rooted_directory(
                    root,
                    diagnostic_root,
                    &parent,
                    true,
                    true,
                ),
                LocalAtomicWriteStage::PrepareParent,
                &requested_path,
                None,
                LocalAtomicDestinationState::Unchanged,
            )?;
        }
        let (destination_existed, preserve_destination_metadata) =
            match read_rooted_symlink_metadata(root, diagnostic_root, path) {
                Ok(file) => {
                    if options.publication_mode()
                        == LocalAtomicPublicationMode::CreateNew
                    {
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
                    } else if options.replaces_target_symlink()
                        && metadata.file_type().is_symlink()
                    {
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
                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    (false, false)
                }
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
            let name =
                try_random_file_name(".qubit-atomic-", None, Some(".tmp"))
                    .map_err(|source| {
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
                LocalRelativePath::new(relative_parent)
                    .and_then(|parent| parent.join(&name))
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
            match open_rooted_native_writer(
                root,
                diagnostic_root,
                &staging_path,
                &write_options,
            ) {
                Ok(file) => {
                    return Ok(Self {
                        path: requested_path,
                        destination: path.clone(),
                        destination_existed,
                        staged_file: WindowsRootedStagedFile {
                            root: staging_root,
                            path: staging_path.clone(),
                            diagnostic_path: diagnostic_root
                                .join(staging_path.as_path()),
                            file: Some(file),
                            armed: true,
                        },
                        preserve_destination_metadata,
                        durability: options.durability(),
                    });
                }
                Err(source)
                    if source.kind() == io::ErrorKind::AlreadyExists =>
                {
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

    /// Synchronizes and atomically replaces the rooted destination.
    ///
    /// Existing metadata is read from the opened destination during this call
    /// and applied to staging before the identity check and replacement.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the replacement and required directory synchronization
    /// complete.
    ///
    /// # Errors
    ///
    /// Returns a structured error when metadata preservation, staging-file
    /// synchronization, replacement, or parent-directory synchronization
    /// fails. Inspect [`LocalAtomicWriteError::destination_state`] to determine
    /// the known post-failure destination outcome.
    #[inline(always)]
    pub fn commit(self) -> Result<(), LocalAtomicWriteError> {
        self.commit_recoverable().map_err(|error| {
            error.into_final_error_with(Self::finalize_failed_commit)
        })
    }

    /// Attempts to commit while retaining a recoverable rooted writer.
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
    /// staging-file synchronization, replacement, or parent-directory
    /// synchronization fails.
    #[cfg_attr(not(any(unix, windows)), allow(unused_mut))]
    pub fn commit_recoverable(
        self,
    ) -> Result<(), LocalAtomicCommitError<Self>> {
        self.commit_recoverable_with_durability().map(|_| ())
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
    #[cfg_attr(not(any(unix, windows)), allow(unused_mut))]
    pub fn commit_recoverable_with_durability(
        mut self,
    ) -> Result<bool, LocalAtomicCommitError<Self>> {
        #[cfg(unix)]
        {
            match self.commit_attempt() {
                Ok(durable) => Ok(durable),
                Err(error) if self.staged_file.is_open() => {
                    Err(LocalAtomicCommitError::new(error, Some(self)))
                }
                Err(error) => Err(LocalAtomicCommitError::new(error, None)),
            }
        }
        #[cfg(windows)]
        {
            if self.durability == LocalDurabilityRequirement::Required {
                return Err(LocalAtomicCommitError::new(
                    unsupported_atomic_error(&self.path),
                    Some(self),
                ));
            }
            match self.commit_attempt_windows() {
                Ok(durable) => Ok(durable),
                Err(error) if self.staged_file.armed => {
                    Err(LocalAtomicCommitError::new(error, Some(self)))
                }
                Err(error) => Err(LocalAtomicCommitError::new(error, None)),
            }
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

    #[cfg(windows)]
    /// Runs one handle-relative Windows commit attempt.
    fn commit_attempt_windows(
        &mut self,
    ) -> Result<bool, LocalAtomicWriteError> {
        let destination = if self.preserve_destination_metadata {
            Some(
                read_rooted_symlink_metadata(
                    &self.staged_file.root,
                    Path::new(""),
                    &self.destination,
                )
                .map_err(|source| {
                    LocalAtomicWriteError::new(
                        LocalAtomicWriteStage::ReadDestinationMetadata,
                        self.path.clone(),
                        Some(self.staged_file.diagnostic_path.clone()),
                        LocalAtomicDestinationState::Unchanged,
                        source,
                    )
                })?,
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
            self.staged_file
                .file()
                .set_permissions(permissions)
                .map_err(|source| {
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
            let current_destination = read_rooted_symlink_metadata(
                &self.staged_file.root,
                Path::new(""),
                &self.destination,
            )
            .map_err(|source| {
                LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::ReadDestinationMetadata,
                    self.path.clone(),
                    Some(self.staged_file.diagnostic_path.clone()),
                    LocalAtomicDestinationState::Unchanged,
                    source,
                )
            })?;
            let opened =
                crate::rooted::Metadata::from_open_file(opened_destination)
                    .map_err(|source| {
                        LocalAtomicWriteError::new(
                            LocalAtomicWriteStage::ReadDestinationMetadata,
                            self.path.clone(),
                            Some(self.staged_file.diagnostic_path.clone()),
                            LocalAtomicDestinationState::Unchanged,
                            source,
                        )
                    })?;
            let current =
                crate::rooted::Metadata::from_open_file(&current_destination)
                    .map_err(|source| {
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

    #[cfg(unix)]
    /// Runs one rooted commit attempt without consuming recoverable staging.
    ///
    /// # Errors
    ///
    /// Returns the structured commit failure. Errors raised before installation
    /// leave the staging handle open for the public recoverable commit API.
    fn commit_attempt(&mut self) -> Result<bool, LocalAtomicWriteError> {
        let destination = self.open_destination_for_commit()?;
        self.preserve_destination_metadata(destination.as_ref())?;
        let file_durable = self.sync_temporary_file()?;
        self.verify_destination_for_commit(destination.as_ref())?;
        let parent_durable = self.install_and_sync_parent()?;
        Ok(file_durable && parent_durable)
    }

    #[cfg(unix)]
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
    fn open_destination_for_commit(
        &mut self,
    ) -> Result<Option<OpenedAtomicDestination>, LocalAtomicWriteError> {
        if !self.preserve_destination_metadata {
            return Ok(None);
        }
        let destination_result = open_rooted_atomic_destination(
            self.staged_file.parent(),
            &self.final_name,
            self.open_retry_timeout,
        );
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
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "rooted atomic destination disappeared",
                ),
            )),
        }
    }

    #[cfg(unix)]
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
        map_atomic_error(
            result,
            LocalAtomicWriteStage::ApplyDestinationMetadata,
            &self.path,
            Some(self.staged_file.diagnostic_path().to_path_buf()),
            LocalAtomicDestinationState::Unchanged,
        )
    }

    #[cfg(unix)]
    /// Synchronizes the rooted staging file before installation.
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
                map_atomic_error(
                    self.staged_file.file().sync_all(),
                    LocalAtomicWriteStage::SyncTemporaryFile,
                    &self.path,
                    Some(self.staged_file.diagnostic_path().to_path_buf()),
                    LocalAtomicDestinationState::Unchanged,
                )?;
                Ok(true)
            }
        }
    }

    #[cfg(unix)]
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
    fn verify_destination_for_commit(
        &mut self,
        destination: Option<&OpenedAtomicDestination>,
    ) -> Result<(), LocalAtomicWriteError> {
        let Some(destination) = destination else {
            return Ok(());
        };
        verify_rooted_atomic_destination_identity(
            &self.final_name,
            destination,
            &self.path,
            &self.staged_file,
        )
    }

    #[cfg(unix)]
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

    #[cfg(windows)]
    /// Finalizes a consuming Windows commit failure.
    fn finalize_failed_commit(
        mut self,
        error: LocalAtomicWriteError,
    ) -> LocalAtomicWriteError {
        if error.destination_state() == LocalAtomicDestinationState::Unchanged {
            error.with_cleanup_error(self.staged_file.cleanup().err())
        } else {
            error
        }
    }

    #[cfg(not(any(unix, windows)))]
    /// Returns an unsupported failure after a non-Unix commit attempt.
    ///
    /// # Parameters
    ///
    /// * `error` - Unsupported rooted atomic-write failure.
    ///
    /// # Returns
    ///
    /// The unchanged unsupported failure.
    #[inline(always)]
    fn finalize_failed_commit(
        self,
        error: LocalAtomicWriteError,
    ) -> LocalAtomicWriteError {
        error
    }

    #[cfg(unix)]
    /// Installs rooted staging and synchronizes the parent descriptor chain.
    ///
    /// # Errors
    ///
    /// Returns the structured installation or recovery error, or a parent
    /// synchronization error after the destination has been replaced.
    fn install_and_sync_parent(
        &mut self,
    ) -> Result<bool, LocalAtomicWriteError> {
        let install_result = install_rooted_atomic_file(
            &mut self.staged_file,
            &self.final_name,
            self.destination_existed,
        );
        if let Err((source, destination_state, staging_state)) = install_result
        {
            return recover_atomic_install_error(
                AtomicInstallRecovery {
                    path: &self.path,
                    temporary_path: self
                        .staged_file
                        .diagnostic_path()
                        .to_path_buf(),
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
                    sync_rooted_parent_chain(
                        staged_file.parent(),
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
        let temporary_path = self.staged_file.diagnostic_path().to_path_buf();
        self.staged_file.disarm();
        match sync_rooted_parent_chain(
            self.staged_file.parent(),
            &self.parent_dirs_to_sync,
        ) {
            Ok(()) => Ok(true),
            Err(_) if self.durability == LocalDurabilityRequirement::Preferred => {
                Ok(false)
            }
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
    fn write_vectored(
        &mut self,
        buffers: &[io::IoSlice<'_>],
    ) -> io::Result<usize> {
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

#[cfg(any(unix, windows))]
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

#[cfg(not(any(unix, windows)))]
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
