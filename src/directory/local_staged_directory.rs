// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsStr;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

#[cfg(all(test, unix))]
impl LocalStagedDirectory {
    pub(crate) fn test_review_tree_sync_orders_files_directories_root_and_parent() {
        sync::test_review_tree_sync_orders_files_directories_root_and_parent();
    }

    pub(crate) fn test_review_tree_sync_stops_after_nested_directory_failure() {
        sync::test_review_tree_sync_stops_after_nested_directory_failure();
    }
}

#[cfg(test)]
pub(crate) fn fail_tree_sync_after(successful_steps: usize) {
    sync::fail_tree_sync_after(successful_steps);
}

#[cfg(test)]
pub(crate) fn install_stage_created_hook(hook: impl FnOnce(&Path) + 'static) -> impl Drop {
    crate::directory::LocalDirectoryHandle::install_stage_created_hook(hook)
}

use super::tree_recovery::LocalTreeRetainedResource;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::directory::LocalDirectoryHandle;
use crate::error::LocalResult;
use crate::observation::LocalFileIdentity;
#[cfg(unix)]
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempCleanupLimits;
use crate::options::LocalTempDirectoryOptions;

const MAX_OWNER_POLICY_ENTRY_HANDLES: usize = 256;

/// A temporary directory retained under a fixed parent authority.
///
/// Use a stage to prepare and validate a tree before publishing it. The stage
/// remains cleanup-owned until it is published or explicitly cleaned up; its
/// retained directory handle keeps operations bound to the creating authority.
/// Tree validation observes entries as it walks and does not provide an atomic
/// snapshot against concurrent changes. On Unix, creation verifies the opened
/// directory identity, but the native create/open sequence still has a
/// residual race with concurrent mutation of the parent directory.
///
/// # Examples
///
/// ```no_run
/// use qubit_local_files::options::{LocalDirectoryOpenOptions, LocalTempDirectoryOptions};
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let directory = filesystem.open_directory_with_options(
///     std::path::Path::new("."), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let mut stage = directory.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// let entries = stage.observe_tree_with_options(&Default::default())?;
/// drop(entries);
/// stage.cleanup()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct LocalStagedDirectory {
    pub(super) owner: Option<LocalTreeRetainedResource>,
    directory: Option<LocalDirectoryHandle>,
    relative_name: PathBuf,
    identity: Option<LocalFileIdentity>,
    private: bool,
    owner_access_policy: Option<crate::policy::LocalOwnerAccessPolicy>,
}

#[path = "local_staged_directory/access.rs"]
mod access;
#[path = "local_staged_directory/publish.rs"]
mod publish;
#[path = "local_staged_directory/sync.rs"]
mod sync;
#[path = "local_staged_directory/write.rs"]
mod write;

impl LocalStagedDirectory {
    pub(crate) fn new(
        owner: LocalTreeRetainedResource,
        directory: LocalDirectoryHandle,
        relative_name: PathBuf,
        identity: Option<LocalFileIdentity>,
        private: bool,
        owner_access_policy: Option<crate::policy::LocalOwnerAccessPolicy>,
    ) -> Self {
        Self {
            owner: Some(owner),
            directory: Some(directory),
            relative_name,
            identity,
            private,
            owner_access_policy,
        }
    }
    pub(in crate::directory) fn validate_owner_policy(
        policy: &crate::policy::LocalOwnerAccessPolicy,
    ) -> LocalResult<()> {
        super::staged::validate_owner_policy(policy)
    }

    pub(in crate::directory) fn stage_name(options: &LocalTempDirectoryOptions) -> LocalResult<std::ffi::OsString> {
        super::staged::stage_name(options)
    }

    pub(in crate::directory) fn create_stage_directory(
        parent: &crate::rooted::Root,
        name: &OsStr,
        owner_policy: Option<&crate::policy::LocalOwnerAccessPolicy>,
    ) -> Result<File, crate::temp::types::NativePrivateCreateError> {
        super::staged::create_stage_directory(parent, name, owner_policy)
    }

    pub(in crate::directory) fn arm_after_reopen_validation(&mut self) {
        self.owner
            .as_mut()
            .expect("reopened stage cleanup owner is present")
            .arm_after_reopen_validation();
    }

    /// Returns the fixed authority for this stage.
    pub fn directory(&self) -> &LocalDirectoryHandle {
        self.directory.as_ref().expect("stage authority is open")
    }
    /// Returns the stage name relative to its creating parent authority.
    pub fn relative_name(&self) -> &OsStr {
        self.relative_name.as_os_str()
    }
    /// Returns the captured native identity, or `None` when the platform did
    /// not provide one.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Returns the exact owner policy retained for this stage, or `None` when
    /// no owner access policy was configured.
    pub const fn owner_access_policy(&self) -> Option<&crate::policy::LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
    /// Sets the limits used by explicit cleanup and the final Drop attempt.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        if let Some(owner) = &mut self.owner {
            owner.set_cleanup_limits(limits);
        }
    }
}

impl Drop for LocalStagedDirectory {
    fn drop(&mut self) {
        drop(self.directory.take());
        if let Some(owner) = self.owner.take() {
            let _ = owner.cleanup();
        }
    }
}

#[cfg(unix)]
pub(in crate::directory) fn open_unix_tree_entry(
    stage: &LocalStagedDirectory,
    relative: &Path,
    kind: crate::LocalFileKind,
) -> LocalResult<File> {
    if kind == crate::LocalFileKind::Directory {
        return stage
            .directory()
            .open_directory(relative, &LocalDirectoryOpenOptions::new())?
            .root_authority()?
            .try_clone_authority()
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenRoot,
                    Some(stage.directory().diagnostic_path().join(relative)),
                    None,
                    source,
                )
            });
    }
    let relative = crate::LocalRelativePath::new(relative).map_err(|source| {
        LocalFileError::from_io(
            LocalFileOperation::ValidateName,
            Some(relative.to_path_buf()),
            None,
            source,
        )
    })?;
    stage
        .directory()
        .root_authority()?
        .open_reader(&relative, &crate::read::OpenOptions::default())
        .map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::OpenRoot,
                Some(stage.directory().diagnostic_path().join(relative.as_path())),
                None,
                source,
            )
        })
}

#[cfg(windows)]
pub(in crate::directory) fn open_windows_tree_entry(
    stage: &LocalStagedDirectory,
    relative: &Path,
    kind: crate::LocalFileKind,
) -> LocalResult<File> {
    open_windows_tree_entry_with_sync(stage, relative, kind, false)
}

#[cfg(windows)]
pub(in crate::directory) fn open_windows_tree_entry_for_sync(
    stage: &LocalStagedDirectory,
    relative: &Path,
    kind: crate::LocalFileKind,
) -> LocalResult<File> {
    open_windows_tree_entry_with_sync(stage, relative, kind, true)
}

#[cfg(windows)]
fn open_windows_tree_entry_with_sync(
    stage: &LocalStagedDirectory,
    relative: &Path,
    kind: crate::LocalFileKind,
    for_sync: bool,
) -> LocalResult<File> {
    let mut current = stage
        .owner
        .as_ref()
        .expect("stage owner exists")
        .open_child_for_access()?;
    let mut components = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => Some(name),
            _ => None,
        })
        .peekable();
    while let Some(name) = components.next() {
        let directory = components.peek().is_some() || kind == crate::LocalFileKind::Directory;
        current = (if for_sync && !directory {
            crate::temp::internal::privacy_windows::open_existing_entry_at_for_sync(&current, name, directory)
        } else {
            crate::temp::internal::privacy_windows::open_existing_entry_at(&current, name, directory)
        })
        .map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::OpenRoot,
                Some(stage.directory().diagnostic_path().join(relative)),
                None,
                source,
            )
        })?;
    }
    Ok(current)
}

fn resource_limit() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read)
}

pub(in crate::directory) fn validate_owner_policy(policy: &crate::policy::LocalOwnerAccessPolicy) -> LocalResult<()> {
    #[cfg(unix)]
    if let Some((directory, file)) = policy.unix_modes() {
        if directory & !0o700 != 0 || file & !0o700 != 0 {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::ValidateName,
            ));
        }
        return Ok(());
    }
    #[cfg(windows)]
    if let Some((directory, file)) = policy.windows_masks() {
        if directory & !0x001f_01ff != 0 || file & !0x001f_01ff != 0 {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::ValidateName,
            ));
        }
        return Ok(());
    }
    Err(LocalFileError::new(
        LocalFileErrorKind::InvalidOptions,
        LocalFileOperation::ValidateName,
    ))
}

pub(in crate::directory) fn stage_name(options: &LocalTempDirectoryOptions) -> LocalResult<std::ffi::OsString> {
    crate::local::try_random_file_name("qubit-stage-", options.prefix(), options.suffix())
        .map(Into::into)
        .map_err(|source| {
            let invalid = source.kind() == std::io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(LocalFileOperation::GenerateName, None, None, source);
            if invalid {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })
}

fn create_private_directory(
    parent: &crate::rooted::Root,
    name: &OsStr,
) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    let parent_file = parent
        .try_clone_authority()
        .map_err(|source| crate::temp::types::NativePrivateCreateError { source, retained: None })?;
    let result = {
        #[cfg(unix)]
        {
            crate::temp::internal::privacy_unix::create_private_directory_at(&parent_file, name)
        }
        #[cfg(windows)]
        {
            crate::temp::internal::privacy_windows::create_private_directory_at(&parent_file, name)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = name;
            Err(crate::temp::types::NativePrivateCreateError {
                source: std::io::Error::new(std::io::ErrorKind::Unsupported, "private directories are unsupported"),
                retained: None,
            })
        }
    }?;
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("lf3-tree-private-directory-after-open") {
        let identity = crate::observation::query::query_observation(
            &result,
            crate::observation::LocalObservationRequirement::Required,
        )
        .ok()
        .and_then(|observation| observation.identity().cloned());
        let retained = parent_file
            .try_clone()
            .ok()
            .map(|parent| crate::temp::types::NativePrivateEntry {
                parent,
                name: name.to_os_string(),
                file: Some(result),
                identity,
                directory: true,
            });
        return Err(crate::temp::types::NativePrivateCreateError {
            source: std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected failure after private stage open",
            ),
            retained,
        });
    }
    Ok(result)
}

pub(in crate::directory) fn create_stage_directory(
    parent: &crate::rooted::Root,
    name: &OsStr,
    policy: Option<&crate::policy::LocalOwnerAccessPolicy>,
) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    let Some(policy) = policy else {
        return create_private_directory(parent, name);
    };
    let parent_file = parent
        .try_clone_authority()
        .map_err(|source| crate::temp::types::NativePrivateCreateError { source, retained: None })?;
    #[cfg(unix)]
    {
        crate::temp::internal::privacy_unix::create_owner_entry_at(&parent_file, name, true, policy)
    }
    #[cfg(windows)]
    {
        crate::temp::internal::privacy_windows::create_owner_entry_at(&parent_file, name, true, policy)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = policy;
        Err(crate::temp::types::NativePrivateCreateError {
            source: std::io::Error::new(std::io::ErrorKind::Unsupported, "owner policies are unsupported"),
            retained: None,
        })
    }
}

fn create_private_file(parent: &File, name: &OsStr) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    #[cfg(unix)]
    {
        crate::temp::internal::privacy_unix::create_private_file_at(parent, name)
    }
    #[cfg(windows)]
    {
        crate::temp::internal::privacy_windows::create_private_file_at(parent, name)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, name);
        Err(crate::temp::types::NativePrivateCreateError {
            source: std::io::Error::new(std::io::ErrorKind::Unsupported, "private files are unsupported"),
            retained: None,
        })
    }
}

fn create_stage_file(
    parent: &File,
    name: &OsStr,
    policy: Option<&crate::policy::LocalOwnerAccessPolicy>,
) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    if let Some(policy) = policy {
        #[cfg(unix)]
        {
            crate::temp::internal::privacy_unix::create_owner_entry_at(parent, name, false, policy)
        }
        #[cfg(windows)]
        {
            crate::temp::internal::privacy_windows::create_owner_entry_at(parent, name, false, policy)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = policy;
            Err(crate::temp::types::NativePrivateCreateError {
                source: std::io::Error::new(std::io::ErrorKind::Unsupported, "owner policies are unsupported"),
                retained: None,
            })
        }
    } else {
        create_private_file(parent, name)
    }
}

fn cleanup_partial_private_file(
    cause: LocalFileError,
    parent: &File,
    name: &OsStr,
    file: File,
    identity: Option<LocalFileIdentity>,
    path: &Path,
) -> LocalFileError {
    let Ok(parent) = parent.try_clone() else {
        return cause;
    };
    let entry = crate::temp::types::NativePrivateEntry {
        parent,
        name: name.to_os_string(),
        file: Some(file),
        identity,
        directory: false,
    };
    let mut recovery = crate::LocalTempRecovery::from_native(path.to_path_buf(), entry);
    match recovery.cleanup() {
        Ok(()) => cause,
        Err(_) => cause.with_temp_recovery(recovery),
    }
}
