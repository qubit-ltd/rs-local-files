// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
#[cfg(any(test, feature = "test-support"))]
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
        if directory_mode_restricts_traversal
            && entries.iter().any(|(path, kind, _)| {
                *kind == crate::LocalFileKind::Directory
                    && entries
                        .iter()
                        .any(|(candidate, _, _)| candidate != path && candidate.starts_with(path))
            })
        {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata),
                State::Preserved,
            ));
        }
        if entries.len() > maximum_entries {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::Preserved,
            ));
        }
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(entries.len())
            .map_err(|_| super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved))?;
        for (path, kind, expected) in &entries {
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
        let mut applied = false;
        for (path, kind, expected, file, _) in &retained {
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
                .observe_tree_with_options(&traversal)
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
        for (path, kind, expected, file, _) in &retained {
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
        crate::temp::internal::privacy_unix::verify_owner_access(&root, true, policy).map_err(|source| {
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
        for (path, kind, _expected, file, sync_capable) in &retained {
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
            #[cfg(all(test, unix))]
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
        Ok((
            crate::outcome::LocalTreeAccessOutcome::with_owner_policy(State::OwnerPolicyApplied, *policy),
            file_synced,
        ))
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
