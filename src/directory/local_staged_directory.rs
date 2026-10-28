// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
use std::ffi::OsStr;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use qubit_io::ByteInput;

#[cfg(test)]
type StageCreatedHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
enum OwnerPolicySyncEvent {
    PreSync(LocalFileIdentity),
    AccessApplied(LocalFileIdentity),
    FinalSync(LocalFileIdentity),
}

#[cfg(test)]
thread_local! {
    static TREE_SYNC_FAILURE_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    static STAGE_CREATED_HOOK: RefCell<Option<StageCreatedHook>> = const { RefCell::new(None) };
    static OWNER_POLICY_SYNC_EVENTS: RefCell<Option<Vec<OwnerPolicySyncEvent>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn fail_tree_sync_after(successful_steps: usize) {
    TREE_SYNC_FAILURE_AFTER.with(|slot| slot.set(Some(successful_steps)));
}

#[cfg(test)]
pub(crate) struct StageCreatedHookGuard;

#[cfg(test)]
impl Drop for StageCreatedHookGuard {
    fn drop(&mut self) {
        STAGE_CREATED_HOOK.with(|slot| slot.borrow_mut().take());
    }
}

#[cfg(test)]
pub(crate) fn install_stage_created_hook(hook: impl FnOnce(&Path) + 'static) -> StageCreatedHookGuard {
    STAGE_CREATED_HOOK.with(|slot| {
        assert!(slot.borrow().is_none(), "stage creation hook already installed");
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    StageCreatedHookGuard
}

#[cfg(test)]
fn run_stage_created_hook(name: &Path) {
    let hook = STAGE_CREATED_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook(name);
    }
}

#[cfg(test)]
fn fail_tree_sync_if_scheduled() -> std::io::Result<()> {
    let fail = TREE_SYNC_FAILURE_AFTER.with(|slot| match slot.get() {
        Some(0) => {
            slot.set(None);
            true
        }
        Some(remaining) => {
            slot.set(Some(remaining - 1));
            false
        }
        None => false,
    });
    if fail {
        Err(std::io::Error::other("injected tree file sync failure"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn record_owner_policy_sync_event(event: OwnerPolicySyncEvent) {
    OWNER_POLICY_SYNC_EVENTS.with(|events| {
        if let Some(events) = events.borrow_mut().as_mut() {
            events.push(event);
        }
    });
}

use super::tree_observer::LocalTreeObserver;
use super::tree_recovery::LocalTreeCleanupError;
use super::tree_recovery::LocalTreeCreateError;
use super::tree_recovery::LocalTreeRecoveryLocation;
use super::tree_recovery::LocalTreeRetainedResource;
use super::tree_validation::LocalTreeEntry;
use super::tree_validation::LocalTreeValidationOptions;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::directory::LocalDirectoryHandle;
use crate::error::LocalResult;
use crate::observation::LocalFileIdentity;
use crate::observation::LocalFileObservation;
use crate::options::LocalCreateDirectoryOptions;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempCleanupLimits;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;
use crate::outcome::LocalTreeMutationState;
use crate::policy::LocalFileSyncRequirement;

const MAX_OWNER_POLICY_ENTRY_HANDLES: usize = 256;

/// A temporary directory retained under a fixed parent authority.
#[derive(Debug)]
pub struct LocalStagedDirectory {
    pub(in crate::directory) owner: Option<LocalTreeRetainedResource>,
    directory: Option<LocalDirectoryHandle>,
    relative_name: PathBuf,
    pub(in crate::directory) identity: Option<LocalFileIdentity>,
    private: bool,
    owner_access_policy: Option<crate::policy::LocalOwnerAccessPolicy>,
}

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
    /// Returns the fixed authority for this stage.
    pub fn directory(&self) -> &LocalDirectoryHandle {
        self.directory.as_ref().expect("stage authority is open")
    }
    /// Returns the stage name relative to its creating parent authority.
    pub fn relative_name(&self) -> &OsStr {
        self.relative_name.as_os_str()
    }
    /// Returns the captured native identity, when available.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Returns the exact owner policy retained for this stage, when configured.
    pub const fn owner_access_policy(&self) -> Option<&crate::policy::LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
    /// Sets the limits used by explicit cleanup and the final Drop attempt.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        if let Some(owner) = &mut self.owner {
            owner.set_cleanup_limits(limits);
        }
    }
    /// Observes the stage directory through its retained handle.
    pub fn observation(&self) -> LocalResult<LocalFileObservation> {
        self.directory().observe()
    }

    /// Creates a new file beneath the stage and streams at most `maximum`
    /// bytes.
    ///
    /// The destination is never replaced. Input bytes consumed before an I/O or
    /// limit error cannot be restored; a successful result carries the
    /// created file's observation.
    pub fn write_file_new<S: ByteInput + ?Sized>(
        &mut self,
        relative: &Path,
        input: &mut S,
        maximum: u64,
    ) -> LocalResult<LocalFileObservation> {
        let observation = if self.private || self.owner_access_policy.is_some() {
            self.write_private_file_new(relative, input, maximum)?
        } else {
            let mut writer = self.directory().open_writer_with_options(
                relative,
                &LocalWriteOptions::new(LocalWriteMode::CreateNew)
                    .with_atomicity(crate::LocalAtomicityRequirement::Required)
                    .with_file_sync(LocalFileSyncRequirement::NotRequired),
            )?;
            let mut total = 0_u64;
            let mut block = [0_u8; 8192];
            loop {
                let count = qubit_io::Input::read(input, &mut block).map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(writer.path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                if count == 0 {
                    break;
                }
                total = total.checked_add(count as u64).ok_or_else(resource_limit)?;
                if total > maximum {
                    let _ = writer.abort();
                    return Err(resource_limit());
                }
                let mut offset = 0;
                while offset < count {
                    let written = qubit_io::Output::write(&mut writer, &block[offset..count]).map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Commit,
                            Some(writer.path().to_path_buf()),
                            None,
                            source,
                        )
                    })?;
                    if written == 0 {
                        return Err(LocalFileError::new(
                            LocalFileErrorKind::DataCorruption,
                            LocalFileOperation::Commit,
                        ));
                    }
                    offset += written;
                }
            }
            if let Err(error) = writer.commit() {
                let (cause, _, retry) = error.into_parts();
                drop(retry);
                return Err(cause);
            }
            self.directory().observe_entry(relative)?
        };
        let identity = observation
            .identity()
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata))?;
        self.owner
            .as_mut()
            .expect("stage cleanup owner is present")
            .record_entry(relative, identity)?;
        Ok(observation)
    }

    fn write_private_file_new<S: ByteInput + ?Sized>(
        &self,
        relative: &Path,
        input: &mut S,
        maximum: u64,
    ) -> LocalResult<LocalFileObservation> {
        let name = relative
            .file_name()
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::ValidateName))?;
        let parent_relative = relative
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if self.owner_access_policy.is_some() && parent_relative != Path::new(".") {
            self.ensure_owner_parent_directories(parent_relative)?;
        }
        let containing = if parent_relative == Path::new(".") {
            self.directory().clone()
        } else {
            self.directory()
                .open_directory(parent_relative, &LocalDirectoryOpenOptions::new())?
        };
        let parent_file = containing.root_authority()?.try_clone_authority().map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(containing.diagnostic_path().to_path_buf()),
                None,
                source,
            )
        })?;
        let file = create_stage_file(&parent_file, name, self.owner_access_policy.as_ref()).map_err(|error| {
            let path = containing.diagnostic_path().join(name);
            let recovery = error
                .retained
                .map(|entry| crate::LocalTempRecovery::from_native(path.clone(), entry));
            let mut cause = LocalFileError::from_io(LocalFileOperation::OpenWriter, Some(path), None, error.source);
            if let Some(recovery) = recovery {
                cause = cause.with_temp_recovery(recovery);
            }
            cause
        })?;
        let path = containing.diagnostic_path().join(name);
        let identity = match crate::observation::query::query_observation(
            &file,
            crate::observation::LocalObservationRequirement::Required,
        ) {
            Ok(observation) => observation.identity().cloned(),
            Err(cause) => {
                return Err(cleanup_partial_private_file(
                    cause,
                    &parent_file,
                    name,
                    file,
                    None,
                    &path,
                ));
            }
        };
        let Some(identity) = identity else {
            return Err(cleanup_partial_private_file(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
                    .with_path(path.clone()),
                &parent_file,
                name,
                file,
                None,
                &path,
            ));
        };
        let mut file = file;
        let mut total = 0_u64;
        let mut block = [0_u8; 8192];
        loop {
            let count = match qubit_io::Input::read(input, &mut block) {
                Ok(count) => count,
                Err(source) => {
                    let cause = LocalFileError::from_io(LocalFileOperation::Read, Some(path.clone()), None, source);
                    return Err(cleanup_partial_private_file(
                        cause,
                        &parent_file,
                        name,
                        file,
                        Some(identity),
                        &path,
                    ));
                }
            };
            if count == 0 {
                break;
            }
            total = match total.checked_add(count as u64) {
                Some(total) => total,
                None => {
                    return Err(cleanup_partial_private_file(
                        resource_limit(),
                        &parent_file,
                        name,
                        file,
                        Some(identity),
                        &path,
                    ));
                }
            };
            if total > maximum {
                return Err(cleanup_partial_private_file(
                    resource_limit(),
                    &parent_file,
                    name,
                    file,
                    Some(identity),
                    &path,
                ));
            }
            if let Err(source) = std::io::Write::write_all(&mut file, &block[..count]) {
                let cause = LocalFileError::from_io(LocalFileOperation::Commit, Some(path.clone()), None, source);
                return Err(cleanup_partial_private_file(
                    cause,
                    &parent_file,
                    name,
                    file,
                    Some(identity),
                    &path,
                ));
            }
        }
        crate::observation::query::query_observation(&file, crate::observation::LocalObservationRequirement::Required)
    }

    fn ensure_owner_parent_directories(&self, parent_relative: &Path) -> LocalResult<()> {
        let policy = self.owner_access_policy.as_ref().expect("owner policy checked");
        let mut path = PathBuf::new();
        for component in parent_relative.components() {
            let std::path::Component::Normal(name) = component else {
                continue;
            };
            let parent_file = if path.as_os_str().is_empty() {
                self.directory().root_authority()?.try_clone_authority()
            } else {
                self.directory()
                    .open_directory(&path, &LocalDirectoryOpenOptions::new())?
                    .root_authority()?
                    .try_clone_authority()
            }
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenRoot,
                    Some(self.directory().diagnostic_path().join(&path)),
                    None,
                    source,
                )
            })?;
            path.push(name);
            let created = {
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
                    let _ = (parent_file, name, policy);
                    Err(crate::temp::types::NativePrivateCreateError {
                        source: std::io::Error::new(std::io::ErrorKind::Unsupported, "owner policies are unsupported"),
                        retained: None,
                    })
                }
            };
            if let Err(error) = created {
                if error.source.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::CreateTempDirectory,
                        Some(self.directory().diagnostic_path().join(&path)),
                        None,
                        error.source,
                    ));
                }
                let existing = self
                    .directory()
                    .open_directory(&path, &LocalDirectoryOpenOptions::new())?
                    .root_authority()?
                    .try_clone_authority()
                    .map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::OpenRoot,
                            Some(self.directory().diagnostic_path().join(&path)),
                            None,
                            source,
                        )
                    })?;
                #[cfg(unix)]
                crate::temp::internal::privacy_unix::verify_owner_access(&existing, true, policy).map_err(
                    |source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(self.directory().diagnostic_path().join(&path)),
                            None,
                            source,
                        )
                    },
                )?;
                #[cfg(windows)]
                crate::temp::internal::privacy_windows::verify_owner_access(&existing, true, policy).map_err(
                    |source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(self.directory().diagnostic_path().join(&path)),
                            None,
                            source,
                        )
                    },
                )?;
            }
        }
        Ok(())
    }

    /// Creates a lazy, recursive, entry-only observer using the supplied
    /// resource ceilings.
    ///
    /// Recursion and symbolic-link handling are fixed for tree observation: a
    /// link is yielded as an entry and its target is never traversed. The
    /// supplied recursion and symlink policies do not change this behavior;
    /// resource ceilings, error policy, and listing depth still control
    /// this display-oriented observer.
    pub fn observe_tree_with_options(&self, options: &LocalListOptions) -> LocalResult<LocalTreeObserver> {
        let recursive = (*options)
            .with_recursive()
            .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject);
        let walker = self.directory().list_with_options(Path::new("."), &recursive)?;
        Ok(LocalTreeObserver::new(self.directory().clone(), walker))
    }

    /// Validates the complete tree against an expected entry set and optional
    /// constraints. Exact paths must be unique; repeated paths return
    /// `InvalidOptions`. Directory and dangling symbolic links are checked as
    /// entries without following their targets. Finite depth limits fail when
    /// an actual entry exceeds the limit instead of silently truncating
    /// validation.
    ///
    /// The observations detect identity changes at checked entries but do not
    /// provide an atomic snapshot against concurrent filesystem mutations.
    ///
    /// Returns `ResourceLimit` if traversal or exact-set indexing exceeds its
    /// budgets, and `RequirementNotMet` when an enabled entry rule or
    /// expected set is violated.
    pub fn verify_entries(&self, expected: &[LocalTreeEntry], options: &LocalTreeValidationOptions) -> LocalResult<()> {
        if options.exact_entries().is_some_and(|configured| configured != expected) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::ValidateName,
            ));
        }
        super::internal::tree_verification::verify_tree(self, Some(expected), options)
    }

    /// Removes the stage after identity checks and returns cleanup failures
    /// with an owner.
    pub fn cleanup(mut self) -> Result<(), LocalTreeCleanupError> {
        drop(self.directory.take());
        self.owner
            .take()
            .expect("staged directory retains its cleanup owner")
            .cleanup()
    }

    /// Explicitly gives up automatic cleanup and returns a path/identity
    /// recovery record.
    pub fn retain_in_place(mut self) -> LocalTreeRecoveryLocation {
        drop(self.directory.take());
        self.owner
            .take()
            .expect("staged directory retains its cleanup owner")
            .retain_in_place()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod owner_policy_sync_order_tests {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::OWNER_POLICY_SYNC_EVENTS;
    use super::OwnerPolicySyncEvent;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTreePublishOptions;
    use crate::policy::LocalFileSyncRequirement;
    use crate::policy::LocalOwnerAccessPolicy;

    struct EventCollectionGuard;

    impl Drop for EventCollectionGuard {
        fn drop(&mut self) {
            OWNER_POLICY_SYNC_EVENTS.with(|events| events.borrow_mut().take());
        }
    }

    #[test]
    fn owner_policy_publish_syncs_the_applied_identity_after_access_verification() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let parent = LocalFileSystem::host()
            .expect("host filesystem")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent authority");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        stage
            .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
            .expect("payload");
        OWNER_POLICY_SYNC_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
        let _event_collection = EventCollectionGuard;

        let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400);
        let outcome = stage
            .publish_new(
                &parent,
                OsStr::new("published"),
                &LocalTreePublishOptions::new()
                    .with_owner_access_policy(policy)
                    .with_file_sync(LocalFileSyncRequirement::Required),
            )
            .expect("publish after final file sync");
        assert!(outcome.file_synced());

        let events = OWNER_POLICY_SYNC_EVENTS.with(|events| events.borrow_mut().take().expect("sync event collection"));
        assert!(matches!(
            events.as_slice(),
            [
                OwnerPolicySyncEvent::PreSync(pre),
                OwnerPolicySyncEvent::AccessApplied(applied),
                OwnerPolicySyncEvent::FinalSync(final_sync),
            ] if pre == applied && applied == final_sync
        ));
        std::fs::set_permissions(scratch.path().join("published"), std::fs::Permissions::from_mode(0o700))
            .expect("restore fixture directory mode");
        std::fs::set_permissions(
            scratch.path().join("published/payload"),
            std::fs::Permissions::from_mode(0o600),
        )
        .expect("restore fixture file mode");
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

impl LocalDirectoryHandle {
    /// Reopens a stage beneath this authority only if its identity still
    /// matches `expected`.
    /// If reopening or validation fails, this method leaves the named stage in
    /// place so the caller can retry, inspect, or clean it up explicitly.
    pub fn open_staged_directory(
        &self,
        relative: &Path,
        expected: &LocalFileIdentity,
        options: &LocalTreeValidationOptions,
    ) -> LocalResult<LocalStagedDirectory> {
        let directory = self.open_directory(relative, &LocalDirectoryOpenOptions::new())?;
        let observation = directory.observe()?;
        if observation.identity() != Some(expected) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
                    .with_path(self.diagnostic_path().join(relative)),
            );
        }
        let private = options.privacy() == crate::policy::LocalPrivacyRequirement::Required;
        if private && options.owner_access_policy().is_none() {
            #[cfg(unix)]
            let file = directory.root_authority()?.try_clone_authority().map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(directory.diagnostic_path().to_path_buf()),
                    None,
                    source,
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata = file.metadata().map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::getuid() } {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Metadata,
                    ));
                }
            }
            #[cfg(windows)]
            {
                let parent_file = self.root_authority()?.try_clone_authority().map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                let access_file =
                    crate::temp::internal::privacy_windows::open_existing_relative(&parent_file, relative, true)
                        .map_err(|source| {
                            LocalFileError::from_io(
                                LocalFileOperation::Metadata,
                                Some(directory.diagnostic_path().to_path_buf()),
                                None,
                                source,
                            )
                        })?;
                let access_observation = crate::observation::query::query_observation(
                    &access_file,
                    crate::observation::LocalObservationRequirement::Required,
                )?;
                if access_observation.identity() != Some(expected) {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::ChangedDuringRead,
                        LocalFileOperation::Metadata,
                    ));
                }
                crate::temp::internal::privacy_windows::verify_current_user_access(&access_file, true, false).map_err(
                    |source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(directory.diagnostic_path().to_path_buf()),
                            None,
                            source,
                        )
                    },
                )?;
            }
        }
        let identity = observation.identity().cloned();
        let path = self.diagnostic_path().join(relative);
        let owner = LocalTreeRetainedResource::new_for_reopen(
            path,
            relative.to_path_buf(),
            identity.clone(),
            self.clone(),
            Some(directory.clone()),
            LocalTempCleanupLimits::new(),
        );
        let mut stage = LocalStagedDirectory::new(
            owner,
            directory,
            relative.to_path_buf(),
            identity,
            private,
            options.owner_access_policy().copied(),
        );
        if let Some(owner) = stage.owner.as_mut() {
            owner.set_owner_access_policy(options.owner_access_policy().copied());
        }
        super::internal::tree_verification::verify_tree(&stage, options.exact_entries(), options)?;
        stage
            .owner
            .as_mut()
            .expect("reopened stage cleanup owner is present")
            .arm_after_reopen_validation();
        Ok(stage)
    }

    /// Creates a uniquely named stage directly beneath this fixed directory
    /// authority. The `parent` and `create_parent` options do not select or
    /// create an alternate parent for this fixed-authority entry point.
    pub fn create_staged_directory(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> Result<LocalStagedDirectory, LocalTreeCreateError> {
        if options.max_attempts() == Some(0) {
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::GenerateName),
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        }
        if let Some(policy) = options.owner_access_policy()
            && let Err(cause) = validate_owner_policy(policy)
        {
            return Err(LocalTreeCreateError::new(
                cause,
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        }
        let parent = self
            .root_authority()
            .map_err(|cause| LocalTreeCreateError::new(cause, None, Vec::new(), LocalTreeMutationState::NotStarted))?;
        let mut created_file = None;
        let mut relative = None;
        let mut attempts = 0_usize;
        while options.max_attempts().is_none_or(|maximum| attempts < maximum) {
            attempts = attempts.saturating_add(1);
            let name = stage_name(options).map_err(|cause| {
                LocalTreeCreateError::new(cause, None, Vec::new(), LocalTreeMutationState::NotStarted)
            })?;
            #[cfg(all(test, feature = "test-support"))]
            if crate::local::take_repeated_test_support("staged-directory-collision") {
                continue;
            }
            if options.privacy() == crate::policy::LocalPrivacyRequirement::Required
                || options.owner_access_policy().is_some()
            {
                match create_stage_directory(parent, &name, options.owner_access_policy()) {
                    Ok(file) => {
                        created_file = Some(file);
                        relative = Some(PathBuf::from(name));
                        break;
                    }
                    Err(error)
                        if error.source.kind() == std::io::ErrorKind::AlreadyExists && error.retained.is_none() =>
                    {
                        continue;
                    }
                    Err(error) => {
                        let path = self.diagnostic_path().join(&name);
                        let mutation_state = if error.retained.is_some() {
                            LocalTreeMutationState::Started
                        } else {
                            LocalTreeMutationState::NotStarted
                        };
                        let identity = error.retained.as_ref().and_then(|entry| entry.identity.clone());
                        let retained = identity
                            .clone()
                            .map(|identity| {
                                LocalTreeRetainedResource::new(
                                    path.clone(),
                                    PathBuf::from(&name),
                                    Some(identity),
                                    self.clone(),
                                    None,
                                    options.cleanup_limits(),
                                )
                                .with_owner_access_policy(options.owner_access_policy().copied())
                            })
                            .into_iter()
                            .collect();
                        let recovery = error
                            .retained
                            .map(|entry| crate::LocalTempRecovery::from_native(path.clone(), entry));
                        let mut cause = LocalFileError::from_io(
                            LocalFileOperation::CreateTempDirectory,
                            Some(path),
                            None,
                            error.source,
                        );
                        if let Some(recovery) = recovery {
                            cause = cause.with_temp_recovery(recovery);
                        }
                        return Err(LocalTreeCreateError::new(cause, None, retained, mutation_state));
                    }
                }
            } else {
                let path = PathBuf::from(&name);
                match self
                    .filesystem()
                    .create_directory_with_options(&path, &LocalCreateDirectoryOptions::new())
                {
                    Ok(_outcome) => {
                        relative = Some(path);
                        break;
                    }
                    Err(error) if error.kind() == LocalFileErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        let state = match error.effect_state() {
                            Some(crate::error::LocalFileEffectState::Unchanged) => LocalTreeMutationState::NotStarted,
                            Some(
                                crate::error::LocalFileEffectState::PartiallyApplied
                                | crate::error::LocalFileEffectState::Applied,
                            ) => LocalTreeMutationState::Started,
                            Some(crate::error::LocalFileEffectState::Indeterminate) | None => {
                                LocalTreeMutationState::Indeterminate
                            }
                        };
                        return Err(LocalTreeCreateError::new(error, None, Vec::new(), state));
                    }
                }
            }
        }
        let Some(relative) = relative else {
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(
                    LocalFileErrorKind::AlreadyExists,
                    LocalFileOperation::CreateTempDirectory,
                ),
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        };
        let path = self.diagnostic_path().join(&relative);
        let created_identity = if let Some(file) = created_file.as_ref() {
            let observation = match crate::observation::query::query_observation(
                file,
                crate::observation::LocalObservationRequirement::Required,
            ) {
                Ok(observation) => observation,
                Err(cause) => {
                    let owner = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        None,
                        self.clone(),
                        None,
                        options.cleanup_limits(),
                    )
                    .with_owner_access_policy(options.owner_access_policy().copied());
                    return Err(LocalTreeCreateError::new(
                        cause,
                        None,
                        vec![owner],
                        LocalTreeMutationState::Started,
                    ));
                }
            };
            match observation.identity().cloned() {
                Some(identity) => Some(identity),
                None => {
                    let owner = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        None,
                        self.clone(),
                        None,
                        options.cleanup_limits(),
                    )
                    .with_owner_access_policy(options.owner_access_policy().copied());
                    return Err(LocalTreeCreateError::new(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                            .with_path(path.clone()),
                        None,
                        vec![owner],
                        LocalTreeMutationState::Started,
                    ));
                }
            }
        } else {
            None
        };
        #[cfg(test)]
        run_stage_created_hook(&relative);
        let directory = match self.open_directory(&relative, &LocalDirectoryOpenOptions::new()) {
            Ok(directory) => directory,
            Err(cause) => {
                let identity = created_identity.clone().or_else(|| {
                    self.observe_entry(&relative)
                        .ok()
                        .and_then(|observation| observation.identity().cloned())
                });
                let resource = LocalTreeRetainedResource::new(
                    path,
                    relative,
                    identity,
                    self.clone(),
                    None,
                    options.cleanup_limits(),
                )
                .with_owner_access_policy(options.owner_access_policy().copied());
                return Err(LocalTreeCreateError::new(
                    cause,
                    None,
                    vec![resource],
                    LocalTreeMutationState::Started,
                ));
            }
        };
        let observation = match directory.observe() {
            Ok(observation) => observation,
            Err(cause) => {
                let identity = created_identity.clone().or_else(|| {
                    self.observe_entry(&relative)
                        .ok()
                        .and_then(|observation| observation.identity().cloned())
                });
                let child = if created_identity.is_some() {
                    None
                } else {
                    Some(directory.clone())
                };
                let resource = LocalTreeRetainedResource::new(
                    path,
                    relative,
                    identity,
                    self.clone(),
                    child,
                    options.cleanup_limits(),
                )
                .with_owner_access_policy(options.owner_access_policy().copied());
                return Err(LocalTreeCreateError::new(
                    cause,
                    None,
                    vec![resource],
                    LocalTreeMutationState::Started,
                ));
            }
        };
        if created_identity
            .as_ref()
            .is_some_and(|identity| observation.identity() != Some(identity))
        {
            let owner = LocalTreeRetainedResource::new(
                path.clone(),
                relative.clone(),
                created_identity.clone(),
                self.clone(),
                None,
                options.cleanup_limits(),
            )
            .with_owner_access_policy(options.owner_access_policy().copied());
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                    .with_path(path),
                None,
                vec![owner],
                LocalTreeMutationState::Indeterminate,
            ));
        }
        let identity = created_identity.or_else(|| observation.identity().cloned());
        let mut owner = LocalTreeRetainedResource::new(
            path,
            relative.clone(),
            identity.clone(),
            self.clone(),
            Some(directory.clone()),
            options.cleanup_limits(),
        );
        owner.set_owner_access_policy(options.owner_access_policy().copied());
        Ok(LocalStagedDirectory::new(
            owner,
            directory,
            relative,
            identity,
            options.privacy() == crate::policy::LocalPrivacyRequirement::Required,
            options.owner_access_policy().copied(),
        ))
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

fn validate_owner_policy(policy: &crate::policy::LocalOwnerAccessPolicy) -> LocalResult<()> {
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

fn stage_name(options: &LocalTempDirectoryOptions) -> LocalResult<std::ffi::OsString> {
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

fn create_stage_directory(
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

impl LocalStagedDirectory {
    /// Applies an access policy without publishing or relinquishing this stage.
    pub fn apply_access_policy(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::tree_recovery::LocalTreeAccessError> {
        self.apply_access_policy_with_options(policy, &LocalListOptions::new())
    }

    /// Applies an access policy using one bounded recursive tree observation.
    pub fn apply_access_policy_with_options(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
        limits: &LocalListOptions,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::tree_recovery::LocalTreeAccessError> {
        self.apply_access_policy_with_sync(policy, limits, LocalFileSyncRequirement::NotRequired)
            .map(|(outcome, _)| outcome)
    }

    /// Applies an exact owner policy to every ordinary entry in the stage and
    /// verifies the resulting tree before reporting success.
    ///
    /// The operation fixes at most 256 child identities and management
    /// handles before changing access. Larger trees return `ResourceLimit`
    /// without mutation. Nonempty trees also require the target directory
    /// permissions to preserve owner read/list and traversal through each
    /// directory whose descendants must be verified; otherwise the method
    /// returns `RequirementNotMet` before mutation. An empty stage can use
    /// a mode or mask that removes all access because its already-open root
    /// handle is sufficient for final verification.
    pub fn apply_owner_access_policy_with_options(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::tree_recovery::LocalTreeAccessError> {
        self.apply_owner_access_policy_with_sync(policy, limits, LocalFileSyncRequirement::NotRequired, false)
            .map(|(outcome, _)| outcome)
    }

    fn apply_owner_access_policy_with_sync(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
        sync_requirement: LocalFileSyncRequirement,
        pre_synced: bool,
    ) -> Result<(crate::outcome::LocalTreeAccessOutcome, bool), super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        validate_owner_policy(policy)
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
        let mut file_synced = sync_requirement != LocalFileSyncRequirement::NotRequired && pre_synced;
        self.owner_access_policy = Some(*policy);
        if let Some(owner) = self.owner.as_mut() {
            owner.set_owner_access_policy(Some(*policy));
        }
        let traversal = limits
            .with_recursive()
            .without_max_depth()
            .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject)
            .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
        let mut entries = Vec::new();
        let mut seen_bytes = 0_usize;
        for item in self
            .observe_tree_with_options(&traversal)
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?
        {
            let item =
                item.map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
            if entries.len() >= MAX_OWNER_POLICY_ENTRY_HANDLES {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
            if limits.max_entries().is_some_and(|maximum| entries.len() >= maximum) {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
            seen_bytes = seen_bytes
                .checked_add(item.relative_path().as_os_str().as_encoded_bytes().len())
                .ok_or_else(|| super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved))?;
            if limits.max_seen_name_bytes().is_some_and(|maximum| seen_bytes > maximum) {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
            if limits
                .max_depth()
                .is_some_and(|maximum| item.relative_path().components().count() > maximum)
            {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
            entries
                .try_reserve(1)
                .map_err(|_| super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved))?;
            entries.push((
                item.relative_path().to_path_buf(),
                item.observation().metadata().kind(),
                item.observation().identity().cloned(),
            ));
        }
        entries.sort_by_key(|(path, kind, _)| {
            (
                if *kind == crate::LocalFileKind::Directory {
                    1_u8
                } else {
                    0_u8
                },
                std::cmp::Reverse(path.components().count()),
            )
        });
        let directory_mode_restricts_traversal = {
            #[cfg(unix)]
            {
                policy
                    .unix_modes()
                    .is_some_and(|(directory_mode, _)| directory_mode & 0o500 != 0o500)
            }
            #[cfg(windows)]
            {
                policy
                    .windows_masks()
                    .is_some_and(|(directory_mask, _)| directory_mask & 0x21 != 0x21)
            }
            #[cfg(not(any(unix, windows)))]
            {
                true
            }
        };
        if !entries.is_empty() && directory_mode_restricts_traversal {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata),
                State::Preserved,
            ));
        }
        if directory_mode_restricts_traversal
            && entries.iter().any(|(path, kind, _)| {
                *kind == crate::LocalFileKind::Directory
                    && entries
                        .iter()
                        .any(|(candidate, _, _)| candidate != path && candidate.starts_with(path))
            })
        {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata),
                State::Preserved,
            ));
        }
        if entries.len() > MAX_OWNER_POLICY_ENTRY_HANDLES {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::Preserved,
            ));
        }
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(entries.len())
            .map_err(|_| super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved))?;
        for (path, kind, expected) in &entries {
            if !matches!(kind, crate::LocalFileKind::File | crate::LocalFileKind::Directory) {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata),
                    State::Preserved,
                ));
            }
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support("owner-policy-replace-before-open") {
                let native = self.directory().diagnostic_path().join(path);
                let mut backup_name = native.as_os_str().to_os_string();
                backup_name.push(".owner-policy-original");
                let backup = PathBuf::from(backup_name);
                std::fs::rename(&native, &backup).map_err(|source| {
                    super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                        State::Preserved,
                    )
                })?;
                std::fs::write(&native, b"replacement").map_err(|source| {
                    super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                        State::Preserved,
                    )
                })?;
            }
            let mut sync_capable = false;
            let file = if *kind == crate::LocalFileKind::File
                && sync_requirement != LocalFileSyncRequirement::NotRequired
            {
                #[cfg(feature = "test-support")]
                let injected_unsupported = crate::local::take_test_support("owner-policy-sync-capability");
                #[cfg(not(feature = "test-support"))]
                let injected_unsupported = false;
                if injected_unsupported {
                    if sync_requirement == LocalFileSyncRequirement::Required {
                        return Err(super::tree_recovery::LocalTreeAccessError::new(
                            LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Commit),
                            State::Preserved,
                        ));
                    }
                    #[cfg(unix)]
                    {
                        open_unix_tree_entry(self, path, *kind)
                    }
                    #[cfg(windows)]
                    {
                        open_windows_tree_entry(self, path, *kind)
                    }
                    #[cfg(not(any(unix, windows)))]
                    {
                        Err(LocalFileError::new(
                            LocalFileErrorKind::Unsupported,
                            LocalFileOperation::Metadata,
                        ))
                    }
                } else {
                    #[cfg(unix)]
                    {
                        sync_capable = true;
                        open_unix_tree_entry(self, path, *kind)
                    }
                    #[cfg(windows)]
                    {
                        match open_windows_tree_entry_for_sync(self, path, *kind) {
                            Ok(file) => {
                                sync_capable = true;
                                Ok(file)
                            }
                            Err(cause) if sync_requirement == LocalFileSyncRequirement::Required => {
                                return Err(super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved));
                            }
                            Err(_) => open_windows_tree_entry(self, path, *kind),
                        }
                    }
                    #[cfg(not(any(unix, windows)))]
                    {
                        Err(LocalFileError::new(
                            LocalFileErrorKind::Unsupported,
                            LocalFileOperation::Metadata,
                        ))
                    }
                }
            } else {
                #[cfg(unix)]
                {
                    open_unix_tree_entry(self, path, *kind)
                }
                #[cfg(windows)]
                {
                    open_windows_tree_entry(self, path, *kind)
                }
                #[cfg(not(any(unix, windows)))]
                {
                    Err(LocalFileError::new(
                        LocalFileErrorKind::Unsupported,
                        LocalFileOperation::Metadata,
                    ))
                }
            }
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
            let current = crate::observation::query::query_observation(
                &file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
            if current.identity() != expected.as_ref() {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Preserved,
                ));
            }
            retained.push((path.clone(), *kind, expected.clone(), file, sync_capable));
        }
        let owner = self.owner.as_ref().expect("stage recovery owner exists");
        let binding_matches = owner
            .binding_matches_identity()
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
        if !binding_matches {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                State::Preserved,
            ));
        }
        let mut applied = false;
        for (path, kind, expected, file, _) in &retained {
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support_on_nth("owner-policy-apply", 2) {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(self.directory().diagnostic_path().join(path)),
                        None,
                        crate::local::test_fault_error(),
                    ),
                    if applied { State::Mixed } else { State::Preserved },
                ));
            }
            let current = crate::observation::query::query_observation(
                file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(|cause| {
                super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied { State::Mixed } else { State::Preserved },
                )
            })?;
            if current.identity() != expected.as_ref() {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    if applied { State::Mixed } else { State::Preserved },
                ));
            }
            let result = {
                #[cfg(unix)]
                {
                    crate::temp::internal::privacy_unix::apply_owner_access(
                        file,
                        *kind == crate::LocalFileKind::Directory,
                        policy,
                    )
                }
                #[cfg(windows)]
                {
                    crate::temp::internal::privacy_windows::apply_owner_access(
                        file,
                        *kind == crate::LocalFileKind::Directory,
                        policy,
                    )
                }
                #[cfg(not(any(unix, windows)))]
                {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "owner policies are unsupported",
                    ))
                }
            };
            if let Err(source) = result {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(self.directory().diagnostic_path().join(path)),
                        None,
                        source,
                    ),
                    if applied { State::Mixed } else { State::Indeterminate },
                ));
            }
            let rebound = self
                .directory()
                .observe_entry(path)
                .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            if rebound.identity() != expected.as_ref() {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
            applied = true;
        }
        let root_observation = self.directory().observe().map_err(|cause| {
            super::tree_recovery::LocalTreeAccessError::new(
                cause,
                if applied { State::Mixed } else { State::Indeterminate },
            )
        })?;
        if root_observation.identity() != self.identity.as_ref() {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                if applied { State::Mixed } else { State::Indeterminate },
            ));
        }
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("owner-policy-replace-stage-before-root") {
            let native = self.directory().diagnostic_path().to_path_buf();
            let mut backup_name = native.as_os_str().to_os_string();
            backup_name.push(".owner-policy-original");
            let backup = PathBuf::from(backup_name);
            std::fs::rename(&native, &backup).map_err(|source| {
                super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                    if applied { State::Mixed } else { State::Preserved },
                )
            })?;
            std::fs::create_dir(&native).map_err(|source| {
                super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                    if applied { State::Mixed } else { State::Preserved },
                )
            })?;
        }
        let binding_matches = self
            .owner
            .as_ref()
            .expect("stage owner exists")
            .binding_matches_identity()
            .map_err(|cause| {
                super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied { State::Mixed } else { State::Preserved },
                )
            })?;
        if !binding_matches {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                if applied { State::Mixed } else { State::Preserved },
            ));
        }
        let root = self
            .directory()
            .root_authority()
            .and_then(|root| {
                root.try_clone_authority()
                    .map_err(|source| LocalFileError::from_io(LocalFileOperation::Metadata, None, None, source))
            })
            .map_err(|cause| {
                super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied { State::Mixed } else { State::Indeterminate },
                )
            })?;
        let root_result = {
            #[cfg(unix)]
            {
                crate::temp::internal::privacy_unix::apply_owner_access(&root, true, policy)
            }
            #[cfg(windows)]
            {
                crate::temp::internal::privacy_windows::apply_owner_access(&root, true, policy)
            }
            #[cfg(not(any(unix, windows)))]
            {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "owner policies are unsupported",
                ))
            }
        };
        if let Err(source) = root_result {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(self.directory().diagnostic_path().to_path_buf()),
                    None,
                    source,
                ),
                if applied { State::Mixed } else { State::Indeterminate },
            ));
        }
        let binding_matches = self
            .owner
            .as_ref()
            .expect("stage owner exists")
            .binding_matches_identity()
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
        if !binding_matches {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                State::Indeterminate,
            ));
        }
        #[cfg(all(feature = "test-support", unix))]
        if crate::local::take_test_support("owner-policy-mutate-before-final-verify") {
            use std::os::unix::fs::PermissionsExt;
            if let Some((path, _, _, _, _)) = retained
                .iter()
                .find(|(_, kind, _, _, _)| *kind == crate::LocalFileKind::File)
            {
                let native = self.directory().diagnostic_path().join(path);
                let mut permissions = std::fs::metadata(&native)
                    .map_err(|source| {
                        super::tree_recovery::LocalTreeAccessError::new(
                            LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                            State::Indeterminate,
                        )
                    })?
                    .permissions();
                permissions.set_mode(0o600);
                std::fs::set_permissions(&native, permissions).map_err(|source| {
                    super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                        State::Indeterminate,
                    )
                })?;
            }
        }
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("owner-policy-replace-before-final-verify")
            && let Some((path, _, _, _, _)) = retained
                .iter()
                .find(|(_, kind, _, _, _)| *kind == crate::LocalFileKind::File)
        {
            let native = self.directory().diagnostic_path().join(path);
            let mut backup_name = native.as_os_str().to_os_string();
            backup_name.push(".owner-policy-original");
            let backup = PathBuf::from(backup_name);
            std::fs::rename(&native, &backup).map_err(|source| {
                super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                    State::Indeterminate,
                )
            })?;
            std::fs::write(&native, b"replacement").map_err(|source| {
                super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                    State::Indeterminate,
                )
            })?;
        }
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("owner-policy-add-root-entry-before-final-verify") {
            let native = self.directory().diagnostic_path().join("added-during-conversion");
            std::fs::write(&native, b"unexpected").map_err(|source| {
                super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                    State::Indeterminate,
                )
            })?;
        }
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("owner-policy-add-nested-entry-before-final-verify") {
            let native = self
                .directory()
                .diagnostic_path()
                .join("nested/added-during-conversion");
            std::fs::write(&native, b"unexpected").map_err(|source| {
                super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                    State::Indeterminate,
                )
            })?;
        }
        // Rewalk the complete tree while the configured directory policy
        // retains the read/list and traversal rights needed for name checks.
        let mut final_entries = Vec::new();
        let mut final_name_bytes = 0_usize;
        if !entries.is_empty() || !directory_mode_restricts_traversal {
            for item in self
                .observe_tree_with_options(&traversal)
                .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?
            {
                let item =
                    item.map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
                if final_entries.len() >= MAX_OWNER_POLICY_ENTRY_HANDLES
                    || limits
                        .max_entries()
                        .is_some_and(|maximum| final_entries.len() >= maximum)
                {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
                final_name_bytes = final_name_bytes
                    .checked_add(item.relative_path().as_os_str().as_encoded_bytes().len())
                    .ok_or_else(|| {
                        super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Indeterminate)
                    })?;
                if limits
                    .max_seen_name_bytes()
                    .is_some_and(|maximum| final_name_bytes > maximum)
                {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
                if limits
                    .max_depth()
                    .is_some_and(|maximum| item.relative_path().components().count() > maximum)
                {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
                final_entries.try_reserve(1).map_err(|_| {
                    super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Indeterminate)
                })?;
                final_entries.push((
                    item.relative_path().to_path_buf(),
                    item.observation().metadata().kind(),
                    item.observation().identity().cloned(),
                ));
            }
            if final_entries.len() != entries.len()
                || final_entries.iter().any(|(path, kind, identity)| {
                    !entries.iter().any(|(expected_path, expected_kind, expected_identity)| {
                        path == expected_path && kind == expected_kind && identity == expected_identity
                    })
                })
            {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
        }
        // The complete namespace set is now checked, then every pre-opened
        // identity-bound handle is checked for exact policy facts.
        for (path, kind, expected, file, _) in &retained {
            let rebound = self
                .directory()
                .observe_entry(path)
                .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            if rebound.identity() != expected.as_ref() {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
            let current = crate::observation::query::query_observation(
                file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            if current.identity() != expected.as_ref() {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
            let result = {
                #[cfg(unix)]
                {
                    crate::temp::internal::privacy_unix::verify_owner_access(
                        file,
                        *kind == crate::LocalFileKind::Directory,
                        policy,
                    )
                }
                #[cfg(windows)]
                {
                    crate::temp::internal::privacy_windows::verify_owner_access(
                        file,
                        *kind == crate::LocalFileKind::Directory,
                        policy,
                    )
                }
                #[cfg(not(any(unix, windows)))]
                {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "owner policies are unsupported",
                    ))
                }
            };
            result.map_err(|source| {
                super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(self.directory().diagnostic_path().join(path)),
                        None,
                        source,
                    ),
                    State::Indeterminate,
                )
            })?;
            if *kind == crate::LocalFileKind::File {
                #[cfg(test)]
                if let Some(expected) = expected {
                    record_owner_policy_sync_event(OwnerPolicySyncEvent::AccessApplied(expected.clone()));
                }
            }
        }
        #[cfg(unix)]
        crate::temp::internal::privacy_unix::verify_owner_access(&root, true, policy).map_err(|source| {
            super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(self.directory().diagnostic_path().to_path_buf()),
                    None,
                    source,
                ),
                State::Indeterminate,
            )
        })?;
        #[cfg(windows)]
        crate::temp::internal::privacy_windows::verify_owner_access(&root, true, policy).map_err(|source| {
            super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(self.directory().diagnostic_path().to_path_buf()),
                    None,
                    source,
                ),
                State::Indeterminate,
            )
        })?;
        for (path, kind, _expected, file, sync_capable) in &retained {
            if *kind != crate::LocalFileKind::File || sync_requirement == LocalFileSyncRequirement::NotRequired {
                continue;
            }
            if !sync_capable {
                file_synced = false;
                if sync_requirement == LocalFileSyncRequirement::Required {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Commit)
                            .with_path(self.directory().diagnostic_path().join(path)),
                        State::OwnerPolicyApplied,
                    ));
                }
                continue;
            }
            #[cfg(test)]
            let sync_result = fail_tree_sync_if_scheduled().and_then(|()| file.sync_all());
            #[cfg(not(test))]
            let sync_result = file.sync_all();
            #[cfg(feature = "test-support")]
            let sync_result = if crate::local::take_test_support("owner-policy-final-sync-failure") {
                Err(crate::local::test_fault_error())
            } else {
                sync_result
            };
            if let Err(source) = sync_result {
                if sync_requirement == LocalFileSyncRequirement::Required {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::from_io(
                            LocalFileOperation::Commit,
                            Some(self.directory().diagnostic_path().join(path)),
                            None,
                            source,
                        ),
                        State::OwnerPolicyApplied,
                    ));
                }
                file_synced = false;
            } else {
                #[cfg(test)]
                if let Some(expected) = _expected {
                    record_owner_policy_sync_event(OwnerPolicySyncEvent::FinalSync(expected.clone()));
                }
            }
        }
        Ok((
            crate::outcome::LocalTreeAccessOutcome::with_owner_policy(State::OwnerPolicyApplied, *policy),
            file_synced,
        ))
    }

    fn apply_access_policy_with_sync(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
        limits: &LocalListOptions,
        sync_requirement: LocalFileSyncRequirement,
    ) -> Result<(crate::outcome::LocalTreeAccessOutcome, bool), super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        if policy == crate::policy::LocalTreeFinalAccess::Preserve {
            return Ok((crate::outcome::LocalTreeAccessOutcome::new(State::Preserved), false));
        }
        #[cfg(windows)]
        {
            self.apply_windows_access_policy(policy, limits, sync_requirement)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = limits;
            let _ = sync_requirement;
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata),
                State::Indeterminate,
            ));
        }
        #[cfg(unix)]
        {
            let mut entries = Vec::new();
            let mut bytes = 0_usize;
            let traversal = limits
                .without_max_depth()
                .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
            let collect = (|| -> LocalResult<()> {
                for item in self.observe_tree_with_options(&traversal)? {
                    let item = item?;
                    let depth = item.relative_path().components().count();
                    bytes = bytes
                        .checked_add(item.relative_path().as_os_str().as_encoded_bytes().len())
                        .ok_or_else(resource_limit)?;
                    if limits.max_seen_name_bytes().is_some_and(|maximum| bytes > maximum)
                        || limits.max_depth().is_some_and(|maximum| depth > maximum)
                    {
                        return Err(resource_limit());
                    }
                    entries.try_reserve(1).map_err(|_| resource_limit())?;
                    entries.push((
                        item.relative_path().to_path_buf(),
                        item.observation().metadata().kind(),
                        item.observation().identity().cloned().ok_or_else(|| {
                            LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                        })?,
                    ));
                }
                Ok(())
            })();
            if let Err(cause) = collect {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    State::Indeterminate,
                ));
            }
            entries.sort_by_key(|(path, kind, _)| {
                (
                    if *kind == crate::LocalFileKind::Directory { 1 } else { 0 },
                    std::cmp::Reverse(path.components().count()),
                )
            });
            let mut applied = 0_usize;
            let mut synced = sync_requirement != LocalFileSyncRequirement::NotRequired;
            for (relative, kind, identity) in &entries {
                let current = match self.directory().observe_entry(relative) {
                    Ok(current) => current,
                    Err(cause) => {
                        return Err(super::tree_recovery::LocalTreeAccessError::new(
                            cause,
                            if applied == 0 {
                                State::Indeterminate
                            } else {
                                State::Mixed
                            },
                        ));
                    }
                };
                if current.identity() != Some(identity) {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                            .with_path(self.directory().diagnostic_path().join(relative)),
                        if applied == 0 {
                            State::Indeterminate
                        } else {
                            State::Mixed
                        },
                    ));
                }
                if *kind == crate::LocalFileKind::Symlink
                    || !matches!(*kind, crate::LocalFileKind::File | crate::LocalFileKind::Directory)
                {
                    continue;
                }
                let file = match open_unix_tree_entry(self, relative, *kind) {
                    Ok(file) => file,
                    Err(cause) => {
                        return Err(super::tree_recovery::LocalTreeAccessError::new(
                            cause,
                            if applied == 0 {
                                State::Indeterminate
                            } else {
                                State::Mixed
                            },
                        ));
                    }
                };
                let opened_observation = match crate::observation::query::query_observation(
                    &file,
                    crate::observation::LocalObservationRequirement::Required,
                ) {
                    Ok(observation) => observation,
                    Err(cause) => {
                        return Err(super::tree_recovery::LocalTreeAccessError::new(
                            cause,
                            if applied == 0 {
                                State::Indeterminate
                            } else {
                                State::Mixed
                            },
                        ));
                    }
                };
                if opened_observation.identity() != Some(identity) {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                            .with_path(self.directory().diagnostic_path().join(relative)),
                        if applied == 0 {
                            State::Indeterminate
                        } else {
                            State::Mixed
                        },
                    ));
                }
                let read_only = policy == crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser;
                if let Err(source) = crate::temp::internal::privacy_unix::apply_current_user_access(
                    &file,
                    *kind == crate::LocalFileKind::Directory,
                    read_only,
                ) {
                    let cause = LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(self.directory().diagnostic_path().join(relative)),
                        None,
                        source,
                    );
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        cause,
                        if applied == 0 {
                            State::Indeterminate
                        } else {
                            State::Mixed
                        },
                    ));
                }
                let verified = match self.directory().observe_entry(relative) {
                    Ok(verified) => verified,
                    Err(cause) => {
                        return Err(super::tree_recovery::LocalTreeAccessError::new(
                            cause,
                            State::Indeterminate,
                        ));
                    }
                };
                let read_only = policy == crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser;
                let expected_mode = match (*kind == crate::LocalFileKind::Directory, read_only) {
                    (true, false) => 0o700,
                    (false, false) => 0o600,
                    (true, true) => 0o500,
                    (false, true) => 0o400,
                };
                if verified.identity() != Some(identity)
                    || verified.metadata().permissions().unix_mode() != Some(expected_mode)
                {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                            .with_path(self.directory().diagnostic_path().join(relative)),
                        State::Indeterminate,
                    ));
                }
                applied += 1;
                if *kind == crate::LocalFileKind::File && sync_requirement != LocalFileSyncRequirement::NotRequired {
                    #[cfg(test)]
                    let sync_result = fail_tree_sync_if_scheduled().and_then(|()| file.sync_all());
                    #[cfg(not(test))]
                    let sync_result = file.sync_all();
                    if let Err(source) = sync_result {
                        if sync_requirement == LocalFileSyncRequirement::Required {
                            return Err(super::tree_recovery::LocalTreeAccessError::new(
                                LocalFileError::from_io(
                                    LocalFileOperation::Commit,
                                    Some(self.directory().diagnostic_path().join(relative)),
                                    None,
                                    source,
                                ),
                                State::Mixed,
                            ));
                        }
                        synced = false;
                    }
                }
            }
            let root_observation = match self.observation() {
                Ok(observation) => observation,
                Err(cause) => {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        cause,
                        if applied == 0 {
                            State::Indeterminate
                        } else {
                            State::Mixed
                        },
                    ));
                }
            };
            if root_observation.identity() != self.identity.as_ref() {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    if applied == 0 {
                        State::Indeterminate
                    } else {
                        State::Mixed
                    },
                ));
            }
            let root_file = match self.directory().root_authority().and_then(|root| {
                root.try_clone_authority().map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(self.directory().diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })
            }) {
                Ok(file) => file,
                Err(cause) => {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        cause,
                        if applied == 0 {
                            State::Indeterminate
                        } else {
                            State::Mixed
                        },
                    ));
                }
            };
            let read_only = policy == crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser;
            if let Err(source) =
                crate::temp::internal::privacy_unix::apply_current_user_access(&root_file, true, read_only)
            {
                let cause = LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(self.directory().diagnostic_path().to_path_buf()),
                    None,
                    source,
                );
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied == 0 {
                        State::Indeterminate
                    } else {
                        State::Mixed
                    },
                ));
            }
            let verified_root = crate::observation::query::query_observation(
                &root_file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(|cause| super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            let expected_root_mode = if read_only { 0o500 } else { 0o700 };
            if verified_root.identity() != self.identity.as_ref()
                || verified_root.metadata().permissions().unix_mode() != Some(expected_root_mode)
            {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
            let state = match policy {
                crate::policy::LocalTreeFinalAccess::CurrentUserOnly => State::CurrentUserOnly,
                crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser => State::ReadOnlyCurrentUser,
                crate::policy::LocalTreeFinalAccess::Preserve => State::Preserved,
            };
            Ok((crate::outcome::LocalTreeAccessOutcome::new(state), synced))
        }
    }

    #[cfg(windows)]
    fn apply_windows_access_policy(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
        limits: &LocalListOptions,
        sync_requirement: LocalFileSyncRequirement,
    ) -> Result<(crate::outcome::LocalTreeAccessOutcome, bool), super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        let mut entries = Vec::new();
        let mut bytes = 0_usize;
        let traversal = limits
            .without_max_depth()
            .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
        let collect = (|| -> LocalResult<()> {
            for item in self.observe_tree_with_options(&traversal)? {
                let item = item?;
                let depth = item.relative_path().components().count();
                bytes = bytes
                    .checked_add(item.relative_path().as_os_str().as_encoded_bytes().len())
                    .ok_or_else(resource_limit)?;
                if limits.max_seen_name_bytes().is_some_and(|maximum| bytes > maximum)
                    || limits.max_depth().is_some_and(|maximum| depth > maximum)
                {
                    return Err(resource_limit());
                }
                entries.try_reserve(1).map_err(|_| resource_limit())?;
                entries.push((
                    item.relative_path().to_path_buf(),
                    item.observation().metadata().kind(),
                    item.observation().identity().cloned().ok_or_else(|| {
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                    })?,
                ));
            }
            Ok(())
        })();
        if let Err(cause) = collect {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                cause,
                State::Indeterminate,
            ));
        }
        entries.sort_by_key(|(path, kind, _)| {
            (
                if *kind == crate::LocalFileKind::Directory { 1 } else { 0 },
                std::cmp::Reverse(path.components().count()),
            )
        });
        let mut applied = 0_usize;
        let mut synced = sync_requirement != LocalFileSyncRequirement::NotRequired;
        for (relative, kind, identity) in &entries {
            if !matches!(*kind, crate::LocalFileKind::File | crate::LocalFileKind::Directory) {
                continue;
            }
            let file = match if sync_requirement == LocalFileSyncRequirement::NotRequired {
                open_windows_tree_entry(self, relative, *kind)
            } else {
                open_windows_tree_entry_for_sync(self, relative, *kind)
            } {
                Ok(file) => file,
                Err(cause) => {
                    return Err(super::tree_recovery::LocalTreeAccessError::new(
                        cause,
                        if applied == 0 {
                            State::Indeterminate
                        } else {
                            State::Mixed
                        },
                    ));
                }
            };
            let observation = crate::observation::query::query_observation(
                &file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(|cause| {
                super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied == 0 {
                        State::Indeterminate
                    } else {
                        State::Mixed
                    },
                )
            })?;
            if observation.identity() != Some(identity) {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                        .with_path(self.directory().diagnostic_path().join(relative)),
                    if applied == 0 {
                        State::Indeterminate
                    } else {
                        State::Mixed
                    },
                ));
            }
            let is_directory = *kind == crate::LocalFileKind::Directory;
            let read_only = policy == crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser;
            if let Err(source) =
                crate::temp::internal::privacy_windows::apply_current_user_access(&file, is_directory, read_only)
            {
                let cause = LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(self.directory().diagnostic_path().join(relative)),
                    None,
                    source,
                );
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied == 0 {
                        State::Indeterminate
                    } else {
                        State::Mixed
                    },
                ));
            }
            applied += 1;
            if !is_directory && sync_requirement != LocalFileSyncRequirement::NotRequired {
                #[cfg(test)]
                let sync_result = fail_tree_sync_if_scheduled().and_then(|()| file.sync_all());
                #[cfg(not(test))]
                let sync_result = file.sync_all();
                if let Err(source) = sync_result {
                    if sync_requirement == LocalFileSyncRequirement::Required {
                        return Err(super::tree_recovery::LocalTreeAccessError::new(
                            LocalFileError::from_io(
                                LocalFileOperation::Commit,
                                Some(self.directory().diagnostic_path().join(relative)),
                                None,
                                source,
                            ),
                            State::Mixed,
                        ));
                    }
                    synced = false;
                }
            }
        }
        let root_file = match self.owner.as_ref().expect("stage owner exists").open_child_for_access() {
            Ok(file) => file,
            Err(cause) => {
                return Err(super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied == 0 {
                        State::Indeterminate
                    } else {
                        State::Mixed
                    },
                ));
            }
        };
        let root_observation = crate::observation::query::query_observation(
            &root_file,
            crate::observation::LocalObservationRequirement::Required,
        )
        .map_err(|cause| {
            super::tree_recovery::LocalTreeAccessError::new(
                cause,
                if applied == 0 {
                    State::Indeterminate
                } else {
                    State::Mixed
                },
            )
        })?;
        if root_observation.identity() != self.identity.as_ref() {
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                if applied == 0 {
                    State::Indeterminate
                } else {
                    State::Mixed
                },
            ));
        }
        if let Err(source) = crate::temp::internal::privacy_windows::apply_current_user_access(
            &root_file,
            true,
            policy == crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser,
        ) {
            let cause = LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(self.directory().diagnostic_path().to_path_buf()),
                None,
                source,
            );
            return Err(super::tree_recovery::LocalTreeAccessError::new(
                cause,
                if applied == 0 {
                    State::Indeterminate
                } else {
                    State::Mixed
                },
            ));
        }
        let state = match policy {
            crate::policy::LocalTreeFinalAccess::CurrentUserOnly => State::CurrentUserOnly,
            crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser => State::ReadOnlyCurrentUser,
            crate::policy::LocalTreeFinalAccess::Preserve => State::Preserved,
        };
        Ok((crate::outcome::LocalTreeAccessOutcome::new(state), synced))
    }

    fn sync_tree_files(&self, limits: &LocalListOptions, requirement: LocalFileSyncRequirement) -> LocalResult<bool> {
        if requirement == LocalFileSyncRequirement::NotRequired {
            return Ok(false);
        }
        let root = self.directory().root_authority()?;
        let mut synced = true;
        let traversal = limits
            .without_max_depth()
            .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
        for item in self.observe_tree_with_options(&traversal)? {
            let item = item?;
            let depth = item.relative_path().components().count();
            if limits.max_depth().is_some_and(|maximum| depth > maximum) {
                return Err(resource_limit());
            }
            if item.observation().metadata().kind() != crate::LocalFileKind::File {
                continue;
            }
            let expected =
                item.observation().identity().cloned().ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                })?;
            let relative = crate::LocalRelativePath::new(item.relative_path()).map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::ValidateName,
                    Some(item.relative_path().to_path_buf()),
                    None,
                    source,
                )
            })?;
            let opened = root
                .open_writer(
                    &relative,
                    &crate::write::OpenOptions::new(crate::write::Mode::OpenExistingAtStart),
                )
                .map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Commit,
                        Some(self.directory().diagnostic_path().join(item.relative_path())),
                        None,
                        source,
                    )
                })?;
            let observation = crate::observation::query::query_observation(
                &opened,
                crate::observation::LocalObservationRequirement::Required,
            )?;
            if observation.identity() != Some(&expected) {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                        .with_path(self.directory().diagnostic_path().join(item.relative_path())),
                );
            }
            #[cfg(test)]
            if let Err(source) = fail_tree_sync_if_scheduled() {
                if requirement == LocalFileSyncRequirement::Required {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Commit,
                        Some(self.directory().diagnostic_path().join(item.relative_path())),
                        None,
                        source,
                    ));
                }
                synced = false;
                continue;
            }
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support("owner-policy-sync-failure") {
                let source = crate::local::test_fault_error();
                if requirement == LocalFileSyncRequirement::Required {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Commit,
                        Some(self.directory().diagnostic_path().join(item.relative_path())),
                        None,
                        source,
                    ));
                }
                synced = false;
                continue;
            }
            if let Err(source) = opened.sync_all() {
                if requirement == LocalFileSyncRequirement::Required {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Commit,
                        Some(self.directory().diagnostic_path().join(item.relative_path())),
                        None,
                        source,
                    ));
                }
                synced = false;
            } else {
                #[cfg(test)]
                record_owner_policy_sync_event(OwnerPolicySyncEvent::PreSync(expected));
            }
        }
        Ok(synced)
    }

    /// Synchronizes every regular file currently contained in this staged directory.
    ///
    /// The stage remains unpublished. `limits` bounds tree traversal, and a
    /// depth limit that would truncate the tree is reported as an error. With
    /// `Required`, a file synchronization failure is an error; with
    /// `Preferred`, the method returns `false` when a file could not be synced.
    /// `NotRequired` performs no file synchronization and returns `false`.
    /// This method does not synchronize the parent namespace.
    ///
    /// # Errors
    /// Returns an error when the staged tree cannot be completely traversed,
    /// its contents change during observation, a resource limit is exceeded,
    /// or a required file synchronization fails.
    pub fn sync_files_with_options(
        &self,
        limits: &LocalListOptions,
        requirement: LocalFileSyncRequirement,
    ) -> LocalResult<bool> {
        self.sync_tree_files(limits, requirement)
    }

    /// Publishes this stage under a single native name without replacing an
    /// entry.
    pub fn publish_new(
        mut self,
        parent: &LocalDirectoryHandle,
        name: &OsStr,
        options: &crate::options::LocalTreePublishOptions,
    ) -> Result<crate::outcome::LocalTreePublishOutcome, super::tree_recovery::LocalTreePublishError> {
        use crate::outcome::LocalTreeAccessState as Access;
        use crate::outcome::LocalTreeMutationState as Mutation;
        use crate::outcome::LocalTreePublication as Publication;
        let invalid_name = || LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::ValidateName);
        let name_path = Path::new(name);
        if options.owner_access_policy().is_some() && options.access() != crate::policy::LocalTreeFinalAccess::Preserve
        {
            return Err(super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::ValidateName),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        if let Some(policy) = options.owner_access_policy()
            && let Err(cause) = validate_owner_policy(policy)
        {
            return Err(super::tree_recovery::LocalTreePublishError::new(
                cause,
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        if let Some(policy) = options.owner_access_policy() {
            self.owner_access_policy = Some(*policy);
        }
        if let (Some(policy), Some(owner)) = (options.owner_access_policy(), self.owner.as_mut()) {
            owner.set_owner_access_policy(Some(*policy));
        }
        if name.is_empty()
            || name == OsStr::new(".")
            || name == OsStr::new("..")
            || name_path.components().count() != 1
            || !matches!(name_path.components().next(), Some(std::path::Component::Normal(_)))
        {
            return Err(super::tree_recovery::LocalTreePublishError::new(
                invalid_name(),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        let parent_matches = match self
            .owner
            .as_ref()
            .expect("stage cleanup owner exists")
            .parent_matches(parent)
        {
            Ok(matches) => matches,
            Err(cause) => {
                return Err(super::tree_recovery::LocalTreePublishError::new(
                    cause,
                    Some(self),
                    Mutation::NotStarted,
                    Publication::NotPublished,
                    Access::Preserved,
                ));
            }
        };
        if !parent_matches {
            return Err(super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::InvalidState, LocalFileOperation::Rename),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        let source = PathBuf::from(self.relative_name());
        let target = PathBuf::from(name);
        let observed = match parent.observe_entry(&source) {
            Ok(observed) => observed,
            Err(cause) => {
                return Err(super::tree_recovery::LocalTreePublishError::new(
                    cause,
                    Some(self),
                    Mutation::NotStarted,
                    Publication::NotPublished,
                    Access::Preserved,
                ));
            }
        };
        if observed.identity() != self.identity.as_ref() || self.identity.is_none() {
            return Err(super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        if let Some(validation) = options.validation()
            && let Err(cause) =
                super::internal::tree_verification::verify_tree(&self, validation.exact_entries(), validation)
        {
            return Err(super::tree_recovery::LocalTreePublishError::new(
                cause,
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        let default_limits = LocalListOptions::new();
        let access_limits = options
            .validation()
            .map_or(&default_limits, LocalTreeValidationOptions::limits);
        let (access_state, file_synced) = if let Some(policy) = options.owner_access_policy() {
            let synced = match self.sync_tree_files(access_limits, options.file_sync()) {
                Ok(synced) => synced,
                Err(cause) => {
                    return Err(super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        Mutation::NotStarted,
                        Publication::NotPublished,
                        Access::Preserved,
                    ));
                }
            };
            match self.apply_owner_access_policy_with_sync(policy, access_limits, options.file_sync(), synced) {
                Ok((outcome, file_synced)) => (outcome.access_state(), file_synced),
                Err(error) => {
                    let (cause, access_state, _) = error.into_parts();
                    return Err(super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        if access_state == Access::Indeterminate {
                            Mutation::Indeterminate
                        } else if access_state == Access::Preserved {
                            Mutation::NotStarted
                        } else {
                            Mutation::Started
                        },
                        Publication::NotPublished,
                        access_state,
                    ));
                }
            }
        } else if options.access() == crate::policy::LocalTreeFinalAccess::Preserve {
            let file_synced = match self.sync_tree_files(access_limits, options.file_sync()) {
                Ok(synced) => synced,
                Err(cause) => {
                    return Err(super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        Mutation::NotStarted,
                        Publication::NotPublished,
                        Access::Preserved,
                    ));
                }
            };
            (Access::Preserved, file_synced)
        } else {
            match self.apply_access_policy_with_sync(options.access(), access_limits, options.file_sync()) {
                Ok((outcome, synced)) => (outcome.access_state(), synced),
                Err(error) => {
                    let (cause, access_state, _) = error.into_parts();
                    return Err(super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        match access_state {
                            Access::Preserved => Mutation::NotStarted,
                            Access::CurrentUserOnly | Access::ReadOnlyCurrentUser | Access::Mixed => Mutation::Started,
                            Access::OwnerPolicyApplied => Mutation::Started,
                            Access::Indeterminate => Mutation::Indeterminate,
                        },
                        Publication::NotPublished,
                        access_state,
                    ));
                }
            }
        };
        let access_mutation = if access_state == Access::Preserved {
            Mutation::NotStarted
        } else {
            Mutation::Started
        };
        #[cfg(feature = "test-support")]
        if options.owner_access_policy().is_some()
            && crate::local::take_test_support("owner-policy-replace-source-before-rename")
        {
            let native = self.directory().diagnostic_path().to_path_buf();
            let mut backup_name = native.as_os_str().to_os_string();
            backup_name.push(".owner-policy-original");
            let backup = PathBuf::from(backup_name);
            if let Err(source) = std::fs::rename(&native, &backup).and_then(|()| std::fs::create_dir(&native)) {
                return Err(super::tree_recovery::LocalTreePublishError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                    Some(self),
                    Mutation::Indeterminate,
                    Publication::NotPublished,
                    access_state,
                ));
            }
        }
        let final_source = match parent.observe_entry(&source) {
            Ok(observation) if observation.identity() == self.identity.as_ref() => true,
            Ok(_) => false,
            Err(cause) => {
                return Err(super::tree_recovery::LocalTreePublishError::new(
                    cause,
                    Some(self),
                    Mutation::Indeterminate,
                    Publication::NotPublished,
                    access_state,
                ));
            }
        };
        if !final_source {
            return Err(super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                Some(self),
                Mutation::Indeterminate,
                Publication::NotPublished,
                access_state,
            ));
        }
        let rename_options = crate::options::LocalRenameOptions::new().with_durability(options.durability());
        match parent
            .filesystem()
            .rename_with_options(&source, &target, &rename_options)
        {
            Ok(outcome) => {
                if let Some(owner) = self.owner.as_mut() {
                    owner.disarm_after_publish();
                }
                self.owner.take();
                drop(self.directory.take());
                let mut result = crate::outcome::LocalTreePublishOutcome::new(
                    Publication::Published,
                    outcome.durable(),
                    file_synced,
                    access_state,
                );
                result.set_owner_access_policy(options.owner_access_policy().copied());
                Ok(result)
            }
            Err(failure) => {
                let (cause, state) = failure.into_parts();
                let (publication, mutation, keep_stage) = match state {
                    crate::outcome::LocalRenameFailureState::Unchanged => {
                        (Publication::NotPublished, access_mutation, true)
                    }
                    crate::outcome::LocalRenameFailureState::Renamed => {
                        (Publication::Published, Mutation::Started, false)
                    }
                    crate::outcome::LocalRenameFailureState::Indeterminate => {
                        (Publication::Indeterminate, Mutation::Indeterminate, false)
                    }
                };
                if keep_stage {
                    Err(super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        mutation,
                        publication,
                        access_state,
                    ))
                } else {
                    let mut retained = self.owner.take();
                    if let Some(owner) = retained.as_mut() {
                        owner.disarm_after_publish();
                    }
                    drop(self.directory.take());
                    Err(super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        None,
                        mutation,
                        publication,
                        access_state,
                    )
                    .with_retained(retained))
                }
            }
        }
    }
}
