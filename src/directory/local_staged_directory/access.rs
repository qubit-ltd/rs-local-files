// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;

use super::super::tree_observer::LocalTreeObserver;
use super::super::tree_recovery::LocalTreeCleanupError;
use super::super::tree_recovery::LocalTreeRecoveryLocation;
use super::super::tree_validation::LocalTreeEntry;
use super::super::tree_validation::LocalTreeValidationOptions;
use super::LocalStagedDirectory;
use super::MAX_OWNER_POLICY_ENTRY_HANDLES;
#[cfg(unix)]
use super::open_unix_tree_entry;
#[cfg(windows)]
use super::open_windows_tree_entry;
#[cfg(windows)]
use super::open_windows_tree_entry_for_sync;
use super::resource_limit;
#[cfg(test)]
use super::sync::OwnerPolicySyncEvent;
#[cfg(test)]
use super::sync::fail_tree_sync_if_scheduled;
#[cfg(test)]
use super::sync::record_owner_policy_sync_event;
use super::validate_owner_policy;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::error::LocalResult;
use crate::observation::LocalFileObservation;
use crate::options::LocalListOptions;
use crate::policy::LocalFileSyncRequirement;

type OwnerPolicyEntry = (
    PathBuf,
    crate::LocalFileKind,
    Option<crate::observation::LocalFileIdentity>,
);
type RetainedOwnerPolicyEntry = (
    PathBuf,
    crate::LocalFileKind,
    Option<crate::observation::LocalFileIdentity>,
    std::fs::File,
    bool,
);

impl LocalStagedDirectory {
    /// Observes the stage directory through its retained handle.
    pub fn observation(&self) -> LocalResult<LocalFileObservation> {
        self.directory().observe()
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
        super::super::internal::tree_verification::verify_tree(self, Some(expected), options)
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
impl LocalStagedDirectory {
    /// Applies an access policy without publishing or relinquishing this stage.
    pub fn apply_access_policy(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::super::tree_recovery::LocalTreeAccessError> {
        self.apply_access_policy_with_options(policy, &LocalListOptions::new())
    }

    /// Applies an access policy using one bounded recursive tree observation.
    pub fn apply_access_policy_with_options(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
        limits: &LocalListOptions,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::super::tree_recovery::LocalTreeAccessError> {
        self.apply_access_policy_with_sync(policy, limits, LocalFileSyncRequirement::NotRequired)
            .map(|(outcome, _)| outcome)
    }

    /// Applies an exact owner policy to every ordinary entry in the stage and
    /// verifies the resulting tree before reporting success.
    ///
    /// The operation fixes child identities and management handles before
    /// changing access. `limits.max_entries()` bounds this collection when
    /// set; otherwise the library limits it to 256 entries. Larger trees
    /// return `ResourceLimit` without mutation. Nonempty trees also require
    /// target directory permissions to preserve owner read/list and traversal
    /// through each directory whose descendants must be verified; otherwise
    /// the method returns `RequirementNotMet` before mutation. An empty
    /// stage can use a mode or mask that removes all access because its
    /// already-open root handle is sufficient for final verification.
    pub fn apply_owner_access_policy_with_options(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::super::tree_recovery::LocalTreeAccessError> {
        self.apply_owner_access_policy_with_sync(policy, limits, LocalFileSyncRequirement::NotRequired, false)
            .map(|(outcome, _)| outcome)
    }

    pub(in crate::directory) fn apply_owner_access_policy_with_sync(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
        sync_requirement: LocalFileSyncRequirement,
        pre_synced: bool,
    ) -> Result<(crate::outcome::LocalTreeAccessOutcome, bool), super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        validate_owner_policy(policy)
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
        let file_synced = sync_requirement != LocalFileSyncRequirement::NotRequired && pre_synced;
        self.owner_access_policy = Some(*policy);
        if let Some(owner) = self.owner.as_mut() {
            owner.set_owner_access_policy(Some(*policy));
        }
        let (entries, traversal, maximum_entries, directory_mode_restricts_traversal) =
            self.collect_owner_policy_entries(policy, limits)?;
        let retained = self.open_and_pin_owner_policy_entries(&entries, sync_requirement)?;
        let root = self.apply_owner_policy_mutations(policy, &retained)?;
        self.verify_owner_policy_entries(
            policy,
            limits,
            &entries,
            &retained,
            &traversal,
            maximum_entries,
            directory_mode_restricts_traversal,
            &root,
        )?;
        let file_synced = self.sync_owner_policy_files(&retained, sync_requirement, file_synced)?;
        Ok((
            crate::outcome::LocalTreeAccessOutcome::with_owner_policy(State::OwnerPolicyApplied, *policy),
            file_synced,
        ))
    }

    /// Scans the staged namespace and enforces the existing pre-mutation
    /// budgets.
    fn collect_owner_policy_entries(
        &self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
    ) -> Result<(Vec<OwnerPolicyEntry>, LocalListOptions, usize, bool), super::super::tree_recovery::LocalTreeAccessError>
    {
        use crate::outcome::LocalTreeAccessState as State;
        let traversal = limits
            .with_recursive()
            .without_max_depth()
            .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject)
            .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
        let maximum_entries = limits.max_entries().unwrap_or(MAX_OWNER_POLICY_ENTRY_HANDLES);
        let mut entries = Vec::new();
        let mut seen_bytes = 0_usize;
        for item in self
            .observe_tree_with_options(&traversal)
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?
        {
            let item =
                item.map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
            if entries.len() >= maximum_entries {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
            seen_bytes = seen_bytes
                .checked_add(item.relative_path().as_os_str().as_encoded_bytes().len())
                .ok_or_else(|| {
                    super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved)
                })?;
            if limits.max_seen_name_bytes().is_some_and(|maximum| seen_bytes > maximum) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
            if limits
                .max_depth()
                .is_some_and(|maximum| item.relative_path().components().count() > maximum)
            {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
            entries.try_reserve(1).map_err(|_| {
                super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved)
            })?;
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
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata),
                State::Preserved,
            ));
        }
        // Any nested-directory match implies a nonempty tree, already rejected above
        // when traversal is restricted.
        if entries.len() > maximum_entries {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::Preserved,
            ));
        }
        Ok((entries, traversal, maximum_entries, directory_mode_restricts_traversal))
    }

    /// Opens each observed entry and pins the identity before the first
    /// mutation.
    fn open_and_pin_owner_policy_entries(
        &self,
        entries: &[OwnerPolicyEntry],
        sync_requirement: LocalFileSyncRequirement,
    ) -> Result<Vec<RetainedOwnerPolicyEntry>, super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(entries.len())
            .map_err(|_| super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved))?;
        for (path, kind, expected) in entries {
            if !matches!(kind, crate::LocalFileKind::File | crate::LocalFileKind::Directory) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    super::super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                        State::Preserved,
                    )
                })?;
                std::fs::write(&native, b"replacement").map_err(|source| {
                    super::super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                        State::Preserved,
                    )
                })?;
            }
            let mut sync_capable = false;
            let file =
                if *kind == crate::LocalFileKind::File && sync_requirement != LocalFileSyncRequirement::NotRequired {
                    #[cfg(feature = "test-support")]
                    let injected_unsupported = crate::local::take_test_support("owner-policy-sync-capability");
                    #[cfg(not(feature = "test-support"))]
                    let injected_unsupported = false;
                    if injected_unsupported {
                        if sync_requirement == LocalFileSyncRequirement::Required {
                            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                                        cause,
                                        State::Preserved,
                                    ));
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
                .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
            let current = crate::observation::query::query_observation(
                &file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
            if current.identity() != expected.as_ref() {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Preserved,
                ));
            }
            retained.push((path.clone(), *kind, expected.clone(), file, sync_capable));
        }
        let owner = self.owner.as_ref().expect("stage recovery owner exists");
        let binding_matches = owner
            .binding_matches_identity()
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
        if !binding_matches {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                State::Preserved,
            ));
        }
        Ok(retained)
    }

    /// Applies entry and root permissions, preserving the original mutation
    /// order.
    fn apply_owner_policy_mutations(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        retained: &[RetainedOwnerPolicyEntry],
    ) -> Result<std::fs::File, super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        let mut applied = false;
        for (path, kind, expected, file, _) in retained {
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support_on_nth("owner-policy-apply", 2) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied { State::Mixed } else { State::Preserved },
                )
            })?;
            if current.identity() != expected.as_ref() {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            if rebound.identity() != expected.as_ref() {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
            applied = true;
        }
        let root_observation = self.directory().observe().map_err(|cause| {
            super::super::tree_recovery::LocalTreeAccessError::new(
                cause,
                if applied { State::Mixed } else { State::Indeterminate },
            )
        })?;
        if root_observation.identity() != self.identity.as_ref() {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                    if applied { State::Mixed } else { State::Preserved },
                )
            })?;
            std::fs::create_dir(&native).map_err(|source| {
                super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied { State::Mixed } else { State::Preserved },
                )
            })?;
        if !binding_matches {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
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
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
        if !binding_matches {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                State::Indeterminate,
            ));
        }
        Ok(root)
    }

    /// Rewalks names and rechecks identities and final policy facts before
    /// syncing.
    #[allow(clippy::too_many_arguments)]
    fn verify_owner_policy_entries(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
        entries: &[OwnerPolicyEntry],
        retained: &[RetainedOwnerPolicyEntry],
        traversal: &LocalListOptions,
        maximum_entries: usize,
        directory_mode_restricts_traversal: bool,
        root: &std::fs::File,
    ) -> Result<(), super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
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
                        super::super::tree_recovery::LocalTreeAccessError::new(
                            LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                            State::Indeterminate,
                        )
                    })?
                    .permissions();
                permissions.set_mode(0o600);
                std::fs::set_permissions(&native, permissions).map_err(|source| {
                    super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native.clone()), None, source),
                    State::Indeterminate,
                )
            })?;
            std::fs::write(&native, b"replacement").map_err(|source| {
                super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                    State::Indeterminate,
                )
            })?;
        }
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("owner-policy-add-root-entry-before-final-verify") {
            let native = self.directory().diagnostic_path().join("added-during-conversion");
            std::fs::write(&native, b"unexpected").map_err(|source| {
                super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
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
                .observe_tree_with_options(traversal)
                .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?
            {
                let item = item.map_err(|cause| {
                    super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate)
                })?;
                if final_entries.len() >= maximum_entries {
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
                final_name_bytes = final_name_bytes
                    .checked_add(item.relative_path().as_os_str().as_encoded_bytes().len())
                    .ok_or_else(|| {
                        super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Indeterminate)
                    })?;
                if limits
                    .max_seen_name_bytes()
                    .is_some_and(|maximum| final_name_bytes > maximum)
                {
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
                if limits
                    .max_depth()
                    .is_some_and(|maximum| item.relative_path().components().count() > maximum)
                {
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
                final_entries.try_reserve(1).map_err(|_| {
                    super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Indeterminate)
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
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
        }
        // The complete namespace set is now checked, then every pre-opened
        // identity-bound handle is checked for exact policy facts.
        for (path, kind, expected, file, _) in retained {
            let rebound = self
                .directory()
                .observe_entry(path)
                .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            if rebound.identity() != expected.as_ref() {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
            let current = crate::observation::query::query_observation(
                file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            if current.identity() != expected.as_ref() {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
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
                #[cfg(all(test, unix))]
                if let Some(expected) = expected {
                    record_owner_policy_sync_event(OwnerPolicySyncEvent::AccessApplied(expected.clone()));
                }
            }
        }
        #[cfg(unix)]
        crate::temp::internal::privacy_unix::verify_owner_access(root, true, policy).map_err(|source| {
            super::super::tree_recovery::LocalTreeAccessError::new(
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
            super::super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(self.directory().diagnostic_path().to_path_buf()),
                    None,
                    source,
                ),
                State::Indeterminate,
            )
        })?;
        Ok(())
    }

    /// Synchronizes pinned regular files after successful policy verification.
    fn sync_owner_policy_files(
        &self,
        retained: &[RetainedOwnerPolicyEntry],
        sync_requirement: LocalFileSyncRequirement,
        mut file_synced: bool,
    ) -> Result<bool, super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        for (path, kind, _expected, file, sync_capable) in retained {
            if *kind != crate::LocalFileKind::File || sync_requirement == LocalFileSyncRequirement::NotRequired {
                continue;
            }
            if !sync_capable {
                file_synced = false;
                if sync_requirement == LocalFileSyncRequirement::Required {
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
        Ok(file_synced)
    }
    pub(in crate::directory) fn apply_access_policy_with_sync(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
        limits: &LocalListOptions,
        sync_requirement: LocalFileSyncRequirement,
    ) -> Result<(crate::outcome::LocalTreeAccessOutcome, bool), super::super::tree_recovery::LocalTreeAccessError> {
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
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                        return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                        return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                        return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                        return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
            let expected_root_mode = if read_only { 0o500 } else { 0o700 };
            if verified_root.identity() != self.identity.as_ref()
                || verified_root.metadata().permissions().unix_mode() != Some(expected_root_mode)
            {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
    ) -> Result<(crate::outcome::LocalTreeAccessOutcome, bool), super::super::tree_recovery::LocalTreeAccessError> {
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
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                super::super::tree_recovery::LocalTreeAccessError::new(
                    cause,
                    if applied == 0 {
                        State::Indeterminate
                    } else {
                        State::Mixed
                    },
                )
            })?;
            if observation.identity() != Some(identity) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                        return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
            super::super::tree_recovery::LocalTreeAccessError::new(
                cause,
                if applied == 0 {
                    State::Indeterminate
                } else {
                    State::Mixed
                },
            )
        })?;
        if root_observation.identity() != self.identity.as_ref() {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
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
}

#[cfg(all(test, target_os = "linux"))]
mod owner_policy_checkpoint_tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::LocalStagedDirectory;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalListOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::outcome::LocalTreeAccessState;
    use crate::policy::LocalFileSyncRequirement;
    use crate::policy::LocalOwnerAccessPolicy;

    fn fixture(with_file: bool) -> (tempfile::TempDir, LocalStagedDirectory, LocalOwnerAccessPolicy) {
        let scratch = tempfile::tempdir().expect("checkpoint scratch directory");
        let parent = LocalFileSystem::host()
            .expect("host filesystem")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent authority");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        if with_file {
            stage
                .write_file_new(Path::new("payload"), &mut &b"original"[..], 8)
                .expect("original tracked file");
        }
        let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
        (scratch, stage, policy)
    }

    #[test]
    fn test_pinning_rejects_a_removed_or_replaced_collected_entry() {
        for replace in [false, true] {
            let (scratch, stage, policy) = fixture(true);
            let root = scratch.path().join(stage.relative_name());
            let (entries, _, _, _) = stage
                .collect_owner_policy_entries(&policy, &LocalListOptions::new())
                .expect("collect original identities");
            fs::rename(root.join("payload"), root.join("original"))
                .expect("retain original fixture outside its collected name");
            if replace {
                fs::write(root.join("payload"), b"replacement").expect("replacement fixture");
            }
            let error = stage
                .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
                .expect_err("pinning cannot accept a stale collected name");
            assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
            assert_eq!(
                if replace {
                    LocalFileErrorKind::ChangedDuringRead
                } else {
                    LocalFileErrorKind::NotFound
                },
                error.cause().kind()
            );
            assert_eq!(
                b"original",
                fs::read(root.join("original")).expect("original bytes").as_slice()
            );
            if replace {
                assert_eq!(
                    b"replacement",
                    fs::read(root.join("payload")).expect("replacement bytes").as_slice()
                );
            }
            let _recovery = stage.retain_in_place();
        }
    }

    #[test]
    fn test_pinning_rejects_a_replaced_root_binding_even_for_an_empty_tree() {
        let (scratch, stage, policy) = fixture(false);
        let root = scratch.path().join(stage.relative_name());
        let (entries, _, _, _) = stage
            .collect_owner_policy_entries(&policy, &LocalListOptions::new())
            .expect("collect empty tree");
        fs::rename(&root, scratch.path().join("original-stage")).expect("retain original stage");
        fs::create_dir(&root).expect("replace stage binding");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("replacement mode");
        let error = stage
            .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
            .expect_err("root binding must still name the retained stage");
        assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.cause().kind());
        assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
        assert_eq!(
            0o755,
            fs::metadata(&root).expect("replacement metadata").permissions().mode() & 0o777
        );
        let _recovery = stage.retain_in_place();
    }

    #[test]
    fn test_mutation_rejects_a_removed_or_replaced_pinned_name() {
        for replace in [false, true] {
            let (scratch, mut stage, policy) = fixture(true);
            let root = scratch.path().join(stage.relative_name());
            let (entries, _, _, _) = stage
                .collect_owner_policy_entries(&policy, &LocalListOptions::new())
                .expect("collect identities");
            let retained = stage
                .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
                .expect("pin original handle");
            fs::rename(root.join("payload"), root.join("original")).expect("retain original pinned entry");
            if replace {
                fs::write(root.join("payload"), b"replacement").expect("replacement fixture");
                fs::set_permissions(root.join("payload"), fs::Permissions::from_mode(0o644)).expect("replacement mode");
            }
            let error = stage
                .apply_owner_policy_mutations(&policy, &retained)
                .expect_err("rebound name must match the handle just mutated");
            assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state());
            assert_eq!(
                if replace {
                    LocalFileErrorKind::ChangedDuringRead
                } else {
                    LocalFileErrorKind::NotFound
                },
                error.cause().kind()
            );
            assert_eq!(
                0o600,
                fs::metadata(root.join("original"))
                    .expect("pinned metadata")
                    .permissions()
                    .mode()
                    & 0o777
            );
            if replace {
                assert_eq!(
                    0o644,
                    fs::metadata(root.join("payload"))
                        .expect("replacement metadata")
                        .permissions()
                        .mode()
                        & 0o777
                );
                assert_eq!(
                    b"replacement",
                    fs::read(root.join("payload")).expect("replacement bytes").as_slice()
                );
            }
            let _recovery = stage.retain_in_place();
        }
    }

    #[test]
    fn test_mutation_root_binding_failure_reports_whether_child_access_changed() {
        for with_file in [false, true] {
            let (scratch, mut stage, policy) = fixture(with_file);
            let root = scratch.path().join(stage.relative_name());
            let (entries, _, _, _) = stage
                .collect_owner_policy_entries(&policy, &LocalListOptions::new())
                .expect("collect identities");
            let retained = stage
                .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
                .expect("pin handles");
            fs::rename(&root, scratch.path().join("original-stage")).expect("retain original root");
            fs::create_dir(&root).expect("replace root binding");
            // The retained authority still resolves the original file. Only the
            // root's parent binding changed before the root mutation checkpoint.
            fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("replacement mode");
            let error = stage
                .apply_owner_policy_mutations(&policy, &retained)
                .expect_err("root mutation cannot proceed through a replaced binding");
            assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.cause().kind());
            assert_eq!(
                if with_file {
                    LocalTreeAccessState::Mixed
                } else {
                    LocalTreeAccessState::Preserved
                },
                error.access_state()
            );
            assert_eq!(
                0o755,
                fs::metadata(&root)
                    .expect("replacement root metadata")
                    .permissions()
                    .mode()
                    & 0o777
            );
            let _recovery = stage.retain_in_place();
        }
    }

    #[test]
    fn test_final_verification_detects_added_removed_and_replaced_entries() {
        for change in ["added", "removed", "replaced"] {
            let (scratch, mut stage, policy) = fixture(true);
            let native = scratch.path().join(stage.relative_name());
            let (entries, traversal, maximum_entries, restricted) = stage
                .collect_owner_policy_entries(&policy, &LocalListOptions::new())
                .expect("collect identities");
            let retained = stage
                .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
                .expect("pin handles");
            let root = stage
                .apply_owner_policy_mutations(&policy, &retained)
                .expect("apply policy");
            match change {
                "added" => fs::write(native.join("unexpected"), b"added").expect("add entry"),
                "removed" => fs::rename(native.join("payload"), scratch.path().join("original"))
                    .expect("retain removed entry outside stage"),
                "replaced" => {
                    fs::rename(native.join("payload"), scratch.path().join("original")).expect("retain original");
                    fs::write(native.join("payload"), b"replacement").expect("replace entry");
                }
                _ => unreachable!("fixed fixture cases"),
            }
            let error = stage
                .verify_owner_policy_entries(
                    &policy,
                    &LocalListOptions::new(),
                    &entries,
                    &retained,
                    &traversal,
                    maximum_entries,
                    restricted,
                    &root,
                )
                .expect_err("final namespace must exactly match the collected identities");
            assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.cause().kind());
            assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state());
            if change == "replaced" {
                assert_eq!(
                    b"replacement",
                    fs::read(native.join("payload")).expect("replacement bytes").as_slice()
                );
            }
            let _recovery = stage.retain_in_place();
        }
    }

    #[test]
    fn test_final_verification_rechecks_file_and_root_access_after_mutations() {
        for change_root in [false, true] {
            let (scratch, mut stage, policy) = fixture(true);
            let native = scratch.path().join(stage.relative_name());
            let (entries, traversal, maximum_entries, restricted) = stage
                .collect_owner_policy_entries(&policy, &LocalListOptions::new())
                .expect("collect identities");
            let retained = stage
                .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
                .expect("pin handles");
            let root = stage
                .apply_owner_policy_mutations(&policy, &retained)
                .expect("apply policy");
            let changed = if change_root {
                native.clone()
            } else {
                native.join("payload")
            };
            fs::set_permissions(
                &changed,
                fs::Permissions::from_mode(if change_root { 0o755 } else { 0o644 }),
            )
            .expect("change applied exact mode");
            let error = stage
                .verify_owner_policy_entries(
                    &policy,
                    &LocalListOptions::new(),
                    &entries,
                    &retained,
                    &traversal,
                    maximum_entries,
                    restricted,
                    &root,
                )
                .expect_err("matching names and identities do not prove final permissions");
            assert_eq!(LocalFileErrorKind::PermissionDenied, error.cause().kind());
            assert_eq!(LocalFileOperation::Metadata, error.cause().operation());
            assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state());
            fs::set_permissions(
                &changed,
                fs::Permissions::from_mode(if change_root { 0o700 } else { 0o600 }),
            )
            .expect("restore fixture policy");
            stage.cleanup().expect("cleanup unchanged identities");
        }
    }

    #[test]
    fn test_owner_policy_sync_failure_scheduler_reports_final_applied_access() {
        for requirement in [LocalFileSyncRequirement::Required, LocalFileSyncRequirement::Preferred] {
            let (_scratch, mut stage, policy) = fixture(true);
            let (entries, _, _, _) = stage
                .collect_owner_policy_entries(&policy, &LocalListOptions::new())
                .expect("collect identities");
            let retained = stage
                .open_and_pin_owner_policy_entries(&entries, requirement)
                .expect("pin sync-capable handles");
            stage
                .apply_owner_policy_mutations(&policy, &retained)
                .expect("apply owner access");
            super::super::sync::fail_tree_sync_after(0);
            let result = stage.sync_owner_policy_files(&retained, requirement, true);
            if requirement == LocalFileSyncRequirement::Required {
                let error = result.expect_err("required final sync fails");
                assert_eq!(LocalFileErrorKind::Io, error.cause().kind());
                assert_eq!(LocalFileOperation::Commit, error.cause().operation());
                assert_eq!(LocalTreeAccessState::OwnerPolicyApplied, error.access_state());
            } else {
                assert!(!result.expect("preferred failure reports incomplete synchronization"));
            }
            assert!(
                stage
                    .sync_owner_policy_files(&retained, requirement, true)
                    .expect("one-shot failure is consumed")
            );
            stage.cleanup().expect("cleanup after sync retry");
        }
    }

    struct OwnerPolicyFdBudget {
        original: libc::rlimit,
        files: Vec<fs::File>,
    }

    impl OwnerPolicyFdBudget {
        fn exhaust_with_slots(slots: usize) -> Self {
            assert!(
                std::env::var_os("QUBIT_OWNER_POLICY_FD_PHASE").is_some(),
                "FD limits belong only to isolated children"
            );
            let mut original = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: The pointer names a valid writable rlimit value.
            assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut original) }, 0);
            let reduced = libc::rlimit {
                rlim_cur: original.rlim_cur.min(128),
                rlim_max: original.rlim_max,
            };
            // SAFETY: Only this isolated child's soft limit changes; the hard limit is
            // preserved.
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &reduced) }, 0);
            let mut budget = Self {
                original,
                files: Vec::new(),
            };
            loop {
                match fs::File::open("/dev/null") {
                    Ok(file) => budget.files.push(file),
                    Err(error) => {
                        assert_eq!(error.raw_os_error(), Some(libc::EMFILE));
                        break;
                    }
                }
            }
            for _ in 0..slots {
                drop(
                    budget
                        .files
                        .pop()
                        .expect("owned filler descriptor available to release"),
                );
            }
            budget
        }
    }

    impl Drop for OwnerPolicyFdBudget {
        fn drop(&mut self) {
            // SAFETY: Restore this child's original soft limit without changing its hard
            // limit.
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &self.original) }, 0);
        }
    }

    #[test]
    fn test_owner_policy_fd_stage_failures_are_isolated_and_recoverable() {
        let binary = std::env::current_exe().expect("current unit test executable");
        let child = "directory::staged::access::owner_policy_checkpoint_tests::test_owner_policy_fd_stage_child";
        for phase in ["collect", "pin", "mutate", "verify"] {
            for slots in 0..=8 {
                let output = std::process::Command::new(&binary)
                    .args(["--exact", child, "--nocapture", "--test-threads=1"])
                    .env("QUBIT_OWNER_POLICY_FD_PHASE", phase)
                    .env("QUBIT_OWNER_POLICY_FD_SLOTS", slots.to_string())
                    .output()
                    .expect("run isolated FD-budget child");
                assert!(
                    output.status.success(),
                    "phase={phase}, slots={slots}:\nstdout={}\nstderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                    "child test must actually run"
                );
            }
        }
    }

    #[test]
    fn test_owner_policy_fd_stage_child() {
        let Ok(phase) = std::env::var("QUBIT_OWNER_POLICY_FD_PHASE") else {
            return;
        };
        let slots: usize = std::env::var("QUBIT_OWNER_POLICY_FD_SLOTS")
            .expect("child slot budget")
            .parse()
            .expect("numeric slot budget");
        assert!(slots <= 8);
        let (scratch, mut stage, policy) = fixture(true);
        let native = scratch.path().join(stage.relative_name());
        fs::create_dir(native.join("nested")).expect("nested fixture directory");
        stage
            .write_file_new(Path::new("nested/second"), &mut &b"second"[..], 6)
            .expect("second tracked payload");
        for relative in ["payload", "nested/second"] {
            fs::set_permissions(native.join(relative), fs::Permissions::from_mode(0o644)).expect("initial file mode");
        }
        for path in [native.clone(), native.join("nested")] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("initial directory mode");
        }
        let limits = LocalListOptions::new();
        let collected = if phase == "collect" {
            None
        } else {
            Some(
                stage
                    .collect_owner_policy_entries(&policy, &limits)
                    .expect("prepare collected identities"),
            )
        };
        let pinned = if matches!(phase.as_str(), "mutate" | "verify") {
            Some(
                stage
                    .open_and_pin_owner_policy_entries(
                        &collected.as_ref().expect("collected phase").0,
                        LocalFileSyncRequirement::NotRequired,
                    )
                    .expect("prepare pinned handles"),
            )
        } else {
            None
        };
        let root = if phase == "verify" {
            Some(
                stage
                    .apply_owner_policy_mutations(&policy, pinned.as_ref().expect("pinned phase"))
                    .expect("prepare applied access"),
            )
        } else {
            None
        };
        let budget = OwnerPolicyFdBudget::exhaust_with_slots(slots);
        let result = match phase.as_str() {
            "collect" => stage.collect_owner_policy_entries(&policy, &limits).map(|_| ()),
            "pin" => stage
                .open_and_pin_owner_policy_entries(
                    &collected.as_ref().expect("collected phase").0,
                    LocalFileSyncRequirement::NotRequired,
                )
                .map(|_| ()),
            "mutate" => stage
                .apply_owner_policy_mutations(&policy, pinned.as_ref().expect("pinned phase"))
                .map(|_| ()),
            "verify" => {
                let (entries, traversal, maximum, restricted) = collected.as_ref().expect("collected phase");
                stage.verify_owner_policy_entries(
                    &policy,
                    &limits,
                    entries,
                    pinned.as_ref().expect("pinned phase"),
                    traversal,
                    *maximum,
                    *restricted,
                    root.as_ref().expect("applied root"),
                )
            }
            _ => unreachable!("fixed phase matrix"),
        };
        drop(budget);
        if let Err(error) = result {
            assert_eq!(
                LocalFileErrorKind::Io,
                error.cause().kind(),
                "phase={phase}, slots={slots}: {error:?}"
            );
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error.cause());
            let mut native_emfile = false;
            while let Some(current) = source {
                if current
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.raw_os_error() == Some(libc::EMFILE))
                {
                    native_emfile = true;
                    break;
                }
                source = current.source();
            }
            assert!(
                native_emfile,
                "phase={phase}, slots={slots} must preserve native EMFILE: {error:?}"
            );
            match phase.as_str() {
                "collect" | "pin" => assert_eq!(LocalTreeAccessState::Preserved, error.access_state()),
                "verify" => assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state()),
                "mutate" => assert!(
                    matches!(
                        error.access_state(),
                        LocalTreeAccessState::Mixed | LocalTreeAccessState::Indeterminate
                    ),
                    "pinned child handles must apply access before the root FD checkpoint"
                ),
                _ => unreachable!("fixed phase matrix"),
            }
        } else if matches!(phase.as_str(), "mutate" | "verify") {
            for path in [native.clone(), native.join("nested")] {
                assert_eq!(
                    0o700,
                    fs::metadata(path)
                        .expect("successful final directory mode")
                        .permissions()
                        .mode()
                        & 0o777
                );
            }
            for relative in ["payload", "nested/second"] {
                assert_eq!(
                    0o600,
                    fs::metadata(native.join(relative))
                        .expect("successful final file mode")
                        .permissions()
                        .mode()
                        & 0o777
                );
            }
        }
        assert_eq!(
            b"original",
            fs::read(native.join("payload"))
                .expect("original data preserved")
                .as_slice()
        );
        assert_eq!(
            b"second",
            fs::read(native.join("nested/second"))
                .expect("nested data preserved")
                .as_slice()
        );
        for relative in ["payload", "nested/second"] {
            let mode = fs::metadata(native.join(relative))
                .expect("file metadata after recovery")
                .permissions()
                .mode()
                & 0o777;
            match phase.as_str() {
                "collect" | "pin" => assert_eq!(0o644, mode, "non-mutating phase preserves file mode"),
                "verify" => assert_eq!(0o600, mode, "verification preserves applied file mode"),
                "mutate" => assert!(
                    matches!(mode, 0o600 | 0o644),
                    "partial mutation keeps a documented file mode: {mode:o}"
                ),
                _ => unreachable!("fixed phase matrix"),
            }
        }
        stage
            .apply_owner_access_policy_with_options(&policy, &limits)
            .expect("complete retry after restoring descriptor capacity");
        for relative in ["payload", "nested/second"] {
            assert_eq!(
                0o600,
                fs::metadata(native.join(relative))
                    .expect("retried file mode")
                    .permissions()
                    .mode()
                    & 0o777
            );
        }
        drop(root);
        drop(pinned);
        stage.cleanup().expect("cleanup after successful full-policy retry");
        assert!(!native.exists());
    }

    #[test]
    fn test_owner_policy_native_faults_at_prepared_stage_boundaries() {
        use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
        use crate::tests::native_io_fault_tests::NativeIoPoint;

        let mut reached = Vec::new();
        for phase in ["collect", "pin", "mutate", "verify", "sync"] {
            for point in [
                NativeIoPoint::CloneAuthority,
                NativeIoPoint::OpenCursor,
                NativeIoPoint::CloneCursor,
                NativeIoPoint::CursorMetadata,
                NativeIoPoint::CursorDescend,
                NativeIoPoint::Observation,
                NativeIoPoint::RootMetadata,
                NativeIoPoint::EntryMetadata,
            ] {
                for occurrence in 1..=64 {
                    let (scratch, mut stage, policy) = fixture(true);
                    let native = scratch.path().join(stage.relative_name());
                    fs::set_permissions(&native, fs::Permissions::from_mode(0o755)).expect("initial root mode");
                    fs::set_permissions(native.join("payload"), fs::Permissions::from_mode(0o644))
                        .expect("initial file mode");
                    let limits = LocalListOptions::new();
                    let collected = if phase == "collect" || phase == "sync" {
                        None
                    } else {
                        Some(
                            stage
                                .collect_owner_policy_entries(&policy, &limits)
                                .expect("prepare collected identities"),
                        )
                    };
                    let retained = if matches!(phase, "mutate" | "verify") {
                        Some(
                            stage
                                .open_and_pin_owner_policy_entries(
                                    &collected.as_ref().expect("collected phase").0,
                                    LocalFileSyncRequirement::Required,
                                )
                                .expect("prepare pinned handles"),
                        )
                    } else {
                        None
                    };
                    let root = if phase == "verify" {
                        Some(
                            stage
                                .apply_owner_policy_mutations(&policy, retained.as_ref().expect("pinned phase"))
                                .expect("prepare applied policy"),
                        )
                    } else {
                        None
                    };
                    let guard = NativeIoFaultGuard::fail_on(point, occurrence);
                    let result = match phase {
                        "collect" => stage.collect_owner_policy_entries(&policy, &limits).map(|_| ()),
                        "pin" => stage
                            .open_and_pin_owner_policy_entries(
                                &collected.as_ref().expect("collected phase").0,
                                LocalFileSyncRequirement::Required,
                            )
                            .map(|_| ()),
                        "mutate" => stage
                            .apply_owner_policy_mutations(&policy, retained.as_ref().expect("pinned phase"))
                            .map(|_| ()),
                        "verify" => {
                            let (entries, traversal, maximum, restricted) =
                                collected.as_ref().expect("collected phase");
                            stage.verify_owner_policy_entries(
                                &policy,
                                &limits,
                                entries,
                                retained.as_ref().expect("pinned phase"),
                                traversal,
                                *maximum,
                                *restricted,
                                root.as_ref().expect("applied root"),
                            )
                        }
                        "sync" => stage
                            .sync_tree_files(&limits, LocalFileSyncRequirement::Required)
                            .map(|_| ())
                            .map_err(|cause| {
                                crate::directory::LocalTreeAccessError::new(cause, LocalTreeAccessState::Preserved)
                            }),
                        _ => unreachable!("fixed phase matrix"),
                    };
                    let fired = guard.fired();
                    drop(guard);
                    if fired {
                        reached.push((phase, point, occurrence));
                        let error =
                            result.expect_err("native fault must prevent the selected stage from claiming success");
                        assert_eq!(
                            LocalFileErrorKind::Io,
                            error.cause().kind(),
                            "{phase}/{point:?}/{occurrence}: {error:?}"
                        );
                        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error.cause());
                        let mut found_injected = false;
                        while let Some(current) = source {
                            if current.downcast_ref::<std::io::Error>().is_some_and(|io| {
                                io.kind() == std::io::ErrorKind::Other
                                    && io.to_string() == "injected native I/O contract failure"
                            }) {
                                found_injected = true;
                                break;
                            }
                            source = current.source();
                        }
                        assert!(
                            found_injected,
                            "structured cause must preserve the injected native source: {error:?}"
                        );
                        match phase {
                            "collect" | "pin" | "sync" => {
                                assert_eq!(LocalTreeAccessState::Preserved, error.access_state())
                            }
                            "verify" => {
                                assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state())
                            }
                            "mutate" => {
                                let expected_state = match (point, occurrence) {
                                    (NativeIoPoint::Observation, 1) => LocalTreeAccessState::Preserved,
                                    (NativeIoPoint::OpenCursor, 1 | 3) => LocalTreeAccessState::Indeterminate,
                                    (NativeIoPoint::OpenCursor, 2)
                                    | (NativeIoPoint::CloneAuthority, 1 | 2)
                                    | (NativeIoPoint::Observation, 2) => LocalTreeAccessState::Mixed,
                                    _ => panic!("unexpected mutation native checkpoint: {point:?}/{occurrence}"),
                                };
                                assert_eq!(expected_state, error.access_state(), "{point:?}/{occurrence}");
                                assert_eq!(LocalFileOperation::Metadata, error.cause().operation());
                            }
                            _ => unreachable!("fixed phase matrix"),
                        }
                        let (operation, path) = match (phase, point, occurrence) {
                            (_, NativeIoPoint::Observation, _) => (LocalFileOperation::Metadata, None),
                            ("mutate", NativeIoPoint::CloneAuthority, 1)
                            | ("mutate", NativeIoPoint::OpenCursor, 1)
                            | ("collect" | "verify" | "sync", NativeIoPoint::OpenCursor, 2)
                            | ("verify", NativeIoPoint::OpenCursor, 3) => {
                                (LocalFileOperation::Metadata, Some(native.clone()))
                            }
                            ("mutate", NativeIoPoint::CloneAuthority, 2) => (LocalFileOperation::Metadata, None),
                            ("mutate", NativeIoPoint::OpenCursor, 2 | 3) | ("pin", NativeIoPoint::OpenCursor, 1) => {
                                (LocalFileOperation::Metadata, Some(scratch.path().to_path_buf()))
                            }
                            ("collect" | "verify" | "sync", NativeIoPoint::OpenCursor, 1) => {
                                (LocalFileOperation::List, Some(std::path::PathBuf::from("/payload")))
                            }
                            ("collect" | "verify" | "sync", NativeIoPoint::RootMetadata, 1 | 2) => {
                                (LocalFileOperation::List, Some(std::path::PathBuf::from("/")))
                            }
                            _ => panic!("unexpected native error context: {phase}/{point:?}/{occurrence}"),
                        };
                        assert_eq!(operation, error.cause().operation(), "{phase}/{point:?}/{occurrence}");
                        assert_eq!(path.as_deref(), error.cause().path(), "{phase}/{point:?}/{occurrence}");
                    } else {
                        result.expect("a phase with no selected native occurrence must complete");
                    }
                    assert_eq!(
                        b"original",
                        fs::read(native.join("payload"))
                            .expect("original data remains")
                            .as_slice()
                    );
                    if matches!(phase, "collect" | "pin" | "sync") {
                        assert_eq!(
                            0o644,
                            fs::metadata(native.join("payload"))
                                .expect("preserved file mode")
                                .permissions()
                                .mode()
                                & 0o777
                        );
                        assert_eq!(
                            0o755,
                            fs::metadata(&native).expect("preserved root mode").permissions().mode() & 0o777
                        );
                    }
                    if phase == "verify" {
                        assert_eq!(
                            0o600,
                            fs::metadata(native.join("payload"))
                                .expect("applied file mode")
                                .permissions()
                                .mode()
                                & 0o777
                        );
                        assert_eq!(
                            0o700,
                            fs::metadata(&native).expect("applied root mode").permissions().mode() & 0o777
                        );
                    }
                    stage
                        .apply_owner_access_policy_with_options(&policy, &limits)
                        .expect("full policy retry after native fault guard is dropped");
                    assert_eq!(
                        0o600,
                        fs::metadata(native.join("payload"))
                            .expect("retried file mode")
                            .permissions()
                            .mode()
                            & 0o777
                    );
                    assert_eq!(
                        0o700,
                        fs::metadata(&native).expect("retried root mode").permissions().mode() & 0o777
                    );
                    drop(root);
                    drop(retained);
                    stage.cleanup().expect("cleanup after recovered policy operation");
                    assert!(!native.exists());
                    if !fired {
                        break;
                    }
                    assert!(occurrence < 64, "native occurrence sweep must reach the phase boundary");
                }
            }
        }
        // These occurrences reach the root observation, pre-root binding,
        // root clone, post-root binding, final namespace and pinned-handle
        // checkpoints that FD exhaustion can hide behind earlier peaks.
        for checkpoint in [
            ("collect", NativeIoPoint::RootMetadata, 1),
            ("pin", NativeIoPoint::Observation, 1),
            ("pin", NativeIoPoint::OpenCursor, 1),
            ("mutate", NativeIoPoint::Observation, 1),
            ("mutate", NativeIoPoint::Observation, 2),
            ("mutate", NativeIoPoint::CloneAuthority, 2),
            ("mutate", NativeIoPoint::OpenCursor, 2),
            ("mutate", NativeIoPoint::OpenCursor, 3),
            ("verify", NativeIoPoint::RootMetadata, 1),
            ("verify", NativeIoPoint::OpenCursor, 3),
            ("verify", NativeIoPoint::Observation, 1),
            ("sync", NativeIoPoint::Observation, 1),
        ] {
            assert!(
                reached.contains(&checkpoint),
                "required prepared checkpoint was not reached: {checkpoint:?}"
            );
        }
    }

    #[test]
    fn test_remaining_owner_policy_validation_preserves_existing_state() {
        let (scratch, mut stage, valid) = fixture(true);
        let native = scratch.path().join(stage.relative_name());
        stage
            .apply_owner_access_policy_with_options(&valid, &LocalListOptions::new())
            .expect("establish valid owner policy");
        for invalid in [
            LocalOwnerAccessPolicy::new(),
            LocalOwnerAccessPolicy::new().with_windows_masks(0x001f_01ff, 0x001f_01ff),
            LocalOwnerAccessPolicy::new().with_unix_modes(0o755, 0o600),
            LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o644),
        ] {
            let error = stage
                .apply_owner_access_policy_with_options(&invalid, &LocalListOptions::new())
                .expect_err("invalid platform owner policy must fail before touching stage state");
            assert_eq!(LocalFileErrorKind::InvalidOptions, error.cause().kind());
            assert_eq!(LocalFileOperation::ValidateName, error.cause().operation());
            assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
            assert_eq!(Some(&valid), stage.owner_access_policy());
            assert_eq!(
                0o600,
                fs::metadata(native.join("payload"))
                    .expect("preserved file metadata")
                    .permissions()
                    .mode()
                    & 0o777
            );
            assert_eq!(
                0o700,
                fs::metadata(&native)
                    .expect("preserved root metadata")
                    .permissions()
                    .mode()
                    & 0o777
            );
            assert_eq!(
                b"original",
                fs::read(native.join("payload")).expect("preserved data").as_slice()
            );
        }
        stage
            .apply_owner_access_policy_with_options(&valid, &LocalListOptions::new())
            .expect("valid policy still succeeds");
        stage.cleanup().expect("cleanup valid retained state");
        assert!(!native.exists());
    }

    #[test]
    fn test_remaining_access_name_byte_budget_exact_boundary_and_retry() {
        use crate::policy::LocalTreeFinalAccess;
        for owner in [false, true] {
            let (scratch, mut stage, policy) = fixture(true);
            let native = scratch.path().join(stage.relative_name());
            fs::set_permissions(native.join("payload"), fs::Permissions::from_mode(0o644)).expect("initial file mode");
            fs::set_permissions(&native, fs::Permissions::from_mode(0o755)).expect("initial root mode");
            let limits = LocalListOptions::new().with_max_seen_name_bytes(6);
            let result = if owner {
                stage.apply_owner_access_policy_with_options(&policy, &limits)
            } else {
                stage.apply_access_policy_with_options(LocalTreeFinalAccess::CurrentUserOnly, &limits)
            };
            let error = result.expect_err("seven-byte payload name exceeds six-byte budget");
            assert_eq!(LocalFileErrorKind::ResourceLimit, error.cause().kind());
            assert_eq!(
                if owner {
                    LocalTreeAccessState::Preserved
                } else {
                    LocalTreeAccessState::Indeterminate
                },
                error.access_state()
            );
            assert_eq!(
                0o644,
                fs::metadata(native.join("payload"))
                    .expect("pre-mutation file mode")
                    .permissions()
                    .mode()
                    & 0o777
            );
            assert_eq!(
                0o755,
                fs::metadata(&native)
                    .expect("pre-mutation root mode")
                    .permissions()
                    .mode()
                    & 0o777
            );
            for budget in [7, 8] {
                let limits = LocalListOptions::new().with_max_seen_name_bytes(budget);
                let result = if owner {
                    stage.apply_owner_access_policy_with_options(&policy, &limits)
                } else {
                    stage.apply_access_policy_with_options(LocalTreeFinalAccess::CurrentUserOnly, &limits)
                }
                .expect("inclusive budget permits full collection and final verification");
                assert_eq!(
                    if owner {
                        LocalTreeAccessState::OwnerPolicyApplied
                    } else {
                        LocalTreeAccessState::CurrentUserOnly
                    },
                    result.access_state()
                );
                assert_eq!(
                    b"original",
                    fs::read(native.join("payload")).expect("data preserved").as_slice()
                );
                assert_eq!(
                    0o600,
                    fs::metadata(native.join("payload"))
                        .expect("final file mode")
                        .permissions()
                        .mode()
                        & 0o777
                );
                assert_eq!(
                    0o700,
                    fs::metadata(&native).expect("final root mode").permissions().mode() & 0o777
                );
            }
            stage.cleanup().expect("cleanup after budget retry");
            assert!(!native.exists());
        }
    }

    #[test]
    fn test_remaining_ordinary_access_root_clone_and_final_observation_failures() {
        use crate::policy::LocalTreeFinalAccess;
        use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
        use crate::tests::native_io_fault_tests::NativeIoPoint;

        for policy in [
            LocalTreeFinalAccess::CurrentUserOnly,
            LocalTreeFinalAccess::ReadOnlyCurrentUser,
        ] {
            for (point, nth) in [(NativeIoPoint::CloneAuthority, 2), (NativeIoPoint::Observation, 3)] {
                let (scratch, mut stage, _) = fixture(true);
                let native = scratch.path().join(stage.relative_name());
                fs::set_permissions(native.join("payload"), fs::Permissions::from_mode(0o644))
                    .expect("initial file mode");
                fs::set_permissions(&native, fs::Permissions::from_mode(0o755)).expect("initial root mode");
                let fault = NativeIoFaultGuard::fail_on(point, nth);
                let error = stage
                    .apply_access_policy(policy)
                    .expect_err("late native fault must prevent a confirmed access outcome");
                assert!(fault.fired(), "selected root checkpoint must execute");
                drop(fault);
                assert_eq!(LocalFileErrorKind::Io, error.cause().kind());
                assert_eq!(LocalFileOperation::Metadata, error.cause().operation());
                assert_eq!(
                    "injected native I/O contract failure",
                    error.cause().io_error().expect("native source retained").to_string()
                );
                let read_only = policy == LocalTreeFinalAccess::ReadOnlyCurrentUser;
                let expected_file_mode = if read_only { 0o400 } else { 0o600 };
                let expected_root_mode = if read_only { 0o500 } else { 0o700 };
                if point == NativeIoPoint::CloneAuthority {
                    assert_eq!(LocalTreeAccessState::Mixed, error.access_state());
                    assert_eq!(Some(native.as_path()), error.cause().path());
                    assert_eq!(
                        0o755,
                        fs::metadata(&native)
                            .expect("root not yet mutated")
                            .permissions()
                            .mode()
                            & 0o777
                    );
                } else {
                    assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state());
                    assert!(
                        error.cause().path().is_none(),
                        "same-handle observation carries no namespace path"
                    );
                    assert_eq!(
                        expected_root_mode,
                        fs::metadata(&native)
                            .expect("root already mutated")
                            .permissions()
                            .mode()
                            & 0o777
                    );
                }
                assert_eq!(
                    expected_file_mode,
                    fs::metadata(native.join("payload"))
                        .expect("child already mutated")
                        .permissions()
                        .mode()
                        & 0o777
                );
                assert_eq!(
                    b"original",
                    fs::read(native.join("payload")).expect("original contents").as_slice()
                );
                let outcome = stage
                    .apply_access_policy(policy)
                    .expect("retry confirms requested access once native fault is dropped");
                assert_eq!(
                    if read_only {
                        LocalTreeAccessState::ReadOnlyCurrentUser
                    } else {
                        LocalTreeAccessState::CurrentUserOnly
                    },
                    outcome.access_state()
                );
                assert_eq!(
                    expected_root_mode,
                    fs::metadata(&native)
                        .expect("retried root metadata")
                        .permissions()
                        .mode()
                        & 0o777
                );
                stage
                    .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
                    .expect("restore writable fixture before cleanup");
                stage.cleanup().expect("cleanup retry owns original identity");
                assert!(!native.exists());
            }
        }
    }
}
