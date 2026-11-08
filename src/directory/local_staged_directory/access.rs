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
use super::internal::OwnerPolicyEntry;
use super::internal::RetainedOwnerPolicyEntry;
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

#[cfg(test)]
thread_local! {
    static OWNER_POLICY_FINAL_INDEX_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNER_POLICY_TEST_DEADLINE_BOUNDARY: std::cell::Cell<Option<OwnerPolicyDeadlineBoundary>> = const { std::cell::Cell::new(None) };
    static OWNER_POLICY_TEST_NOW: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerPolicyDeadlineBoundary {
    IndexBuild,
    FinalMatch,
    PinnedHandle,
}

#[cfg(test)]
fn reach_owner_policy_deadline_boundary(boundary: OwnerPolicyDeadlineBoundary, started_at: std::time::Instant) {
    OWNER_POLICY_TEST_DEADLINE_BOUNDARY.with(|selected| {
        if selected.get() == Some(boundary) {
            OWNER_POLICY_TEST_NOW.with(|now| now.set(Some(started_at + std::time::Duration::from_secs(3600))));
        }
    });
}

/// Reports whether owner-policy work has consumed the configured elapsed-time
/// budget.
///
/// The duration is measured from the operation's shared start instant so
/// collection, mutation, verification, and synchronization use one deadline.
///
/// # Parameters
///
/// * `limits` — listing options containing the optional elapsed-time limit.
/// * `started_at` — instant from which elapsed time is measured.
///
/// # Returns
///
/// Returns whether the configured elapsed-time budget has expired.
fn owner_policy_deadline_exceeded(limits: &LocalListOptions, started_at: std::time::Instant) -> bool {
    limits.deadline().is_some_and(|deadline| {
        #[cfg(test)]
        let now = OWNER_POLICY_TEST_NOW
            .with(|now| now.get())
            .unwrap_or_else(std::time::Instant::now);
        #[cfg(not(test))]
        let now = std::time::Instant::now();
        now.saturating_duration_since(started_at) >= deadline
    })
}

impl LocalStagedDirectory {
    /// Creates a lazy, recursive, entry-only observer using the supplied
    /// resource ceilings.
    ///
    /// Recursion and symbolic-link handling are fixed for tree observation: a
    /// link is yielded as an entry and its target is never traversed. The
    /// supplied recursion and symlink policies do not change this behavior;
    /// resource ceilings, error policy, and listing depth still control
    /// this display-oriented observer.
    ///
    /// # Parameters
    ///
    /// * `options` — resource ceilings and listing behavior for the observer.
    ///
    /// # Returns
    ///
    /// Returns a lazy observer that yields entries under the retained stage
    /// authority.
    ///
    /// # Errors
    ///
    /// Returns the listing setup error if the stage cannot open its initial
    /// traversal resources.
    pub fn observe_tree_with_options(&self, options: &LocalListOptions) -> LocalResult<LocalTreeObserver> {
        self.observe_tree_with_options_started_at(options, std::time::Instant::now())
    }

    /// Observes the stage directory through its retained handle.
    ///
    /// # Returns
    ///
    /// Returns the current root observation.
    ///
    /// # Errors
    ///
    /// Returns the metadata observation error if the retained handle cannot be
    /// queried.
    pub fn observation(&self) -> LocalResult<LocalFileObservation> {
        self.directory().observe()
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
    ///
    /// Validation failures carry the best available entry path in the
    /// filesystem's public namespace and a stable reason. Resource-limit
    /// failures include typed budget facts when a concrete limit acquisition
    /// was measured. These diagnostic paths describe the failed entry; they
    /// are not authority and are never used to perform validation I/O.
    ///
    /// A successful return means the checked sequence of observations met the
    /// requested constraints. It does not make the observations atomic with
    /// respect to concurrent changes.
    ///
    /// # Parameters
    ///
    /// * `expected` — expected entry set; exact sets must contain unique paths.
    /// * `options` — validation rules and resource limits.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when the checked observations satisfy the constraints.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOptions` for repeated exact paths, `ResourceLimit` when
    /// configured budgets are exceeded, or `RequirementNotMet` when a rule
    /// or expected entry set is violated.
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
    ///
    /// A stage made read-only by `ReadOnlyCurrentUser` may need its access
    /// restored first through [`Self::apply_access_policy`] while this retained
    /// authority is available. On Windows, restoring the DACL does not clear
    /// an existing `FILE_ATTRIBUTE_READONLY`. See the guide's read-only staged
    /// tree recovery example.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` after the stage has been removed.
    ///
    /// # Errors
    ///
    /// Returns cleanup failures together with their owning resource context.
    pub fn cleanup(mut self) -> Result<(), LocalTreeCleanupError> {
        drop(self.directory.take());
        self.owner
            .take()
            .expect("staged directory retains its cleanup owner")
            .cleanup()
    }

    /// Explicitly gives up automatic cleanup and returns a path/identity
    /// recovery record.
    ///
    /// # Returns
    ///
    /// Returns the recorded namespace path and identity after cleanup ownership
    /// has been relinquished.
    #[must_use = "retain the recovery location so the staged data can still be found"]
    pub fn retain_in_place(mut self) -> LocalTreeRecoveryLocation {
        drop(self.directory.take());
        self.owner
            .take()
            .expect("staged directory retains its cleanup owner")
            .retain_in_place()
    }

    /// Builds the same lazy tree observer while preserving a caller's start
    /// time.
    ///
    /// The shared instant makes deadline accounting include setup performed
    /// before traversal begins.
    ///
    /// # Parameters
    ///
    /// * `options` — resource ceilings and listing behavior for the observer.
    /// * `started_at` — instant from which the operation's elapsed budget runs.
    ///
    /// # Returns
    ///
    /// Returns a lazy observer rooted in the retained stage authority.
    pub(crate) fn observe_tree_with_options_started_at(
        &self,
        options: &LocalListOptions,
        started_at: std::time::Instant,
    ) -> LocalResult<LocalTreeObserver> {
        let recursive = (*options)
            .with_recursive()
            .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject);
        let walker = self
            .directory()
            .list_with_options_started_at(Path::new("."), &recursive, started_at)?;
        Ok(LocalTreeObserver::new(self.directory().clone(), walker))
    }
}
impl LocalStagedDirectory {
    /// Applies an access policy without publishing or relinquishing this stage.
    ///
    /// Use this method through the retained stage authority to restore cleanup
    /// access after a read-only publication fails. On Windows, DACL changes do
    /// not clear an existing `FILE_ATTRIBUTE_READONLY`; see the guide's
    /// read-only staged tree recovery example for the supported recovery flow.
    ///
    /// # Parameters
    ///
    /// * `policy` — access policy to apply to ordinary stage entries.
    ///
    /// # Returns
    ///
    /// Returns the observed access outcome after applying the policy.
    ///
    /// # Errors
    ///
    /// Returns an access error with the best-known stage state if mutation or
    /// verification cannot be completed.
    pub fn apply_access_policy(
        &mut self,
        policy: crate::policy::LocalTreeFinalAccess,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::super::tree_recovery::LocalTreeAccessError> {
        self.apply_access_policy_with_options(policy, &LocalListOptions::new())
    }

    /// Applies an access policy using one bounded recursive tree observation.
    ///
    /// # Parameters
    ///
    /// * `policy` — access policy to apply to ordinary stage entries.
    /// * `limits` — resource ceilings for the recursive observation.
    ///
    /// # Returns
    ///
    /// Returns the observed access outcome after applying the policy.
    ///
    /// # Errors
    ///
    /// Returns an access error with the best-known stage state if a resource
    /// limit, mutation, synchronization, or verification fails.
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
    ///
    /// # Parameters
    ///
    /// * `policy` — exact owner permissions to apply and verify.
    /// * `limits` — bounds for collection and final verification.
    ///
    /// # Returns
    ///
    /// Returns the applied access outcome, or an error carrying the best-known
    /// access state when preparation, mutation, or verification fails.
    ///
    /// # Errors
    ///
    /// Returns an access error with the state preserved, mixed, indeterminate,
    /// or applied at the point the operation stopped.
    pub fn apply_owner_access_policy_with_options(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
    ) -> Result<crate::outcome::LocalTreeAccessOutcome, super::super::tree_recovery::LocalTreeAccessError> {
        self.apply_owner_access_policy_with_sync(policy, limits, LocalFileSyncRequirement::NotRequired, false)
            .map(|(outcome, _)| outcome)
    }

    /// Applies an owner policy while carrying the synchronization state across
    /// publication stages.
    ///
    /// This crate-internal entry point lets the staged publication flow avoid
    /// repeating a sync already completed by its caller.
    ///
    /// # Parameters
    ///
    /// * `policy` — exact owner policy to validate and apply.
    /// * `limits` — resource ceilings for every phase.
    /// * `sync_requirement` — whether pinned regular files must be
    ///   synchronized.
    /// * `pre_synced` — whether an earlier publication phase already synced
    ///   files.
    ///
    /// # Returns
    ///
    /// Returns the applied owner-policy outcome and whether required files are
    /// synchronized.
    ///
    /// # Errors
    ///
    /// Returns an access error with the best-known stage state when a phase
    /// fails or the shared deadline expires.
    pub(in crate::directory) fn apply_owner_access_policy_with_sync(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
        sync_requirement: LocalFileSyncRequirement,
        pre_synced: bool,
    ) -> Result<(crate::outcome::LocalTreeAccessOutcome, bool), super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        let started_at = std::time::Instant::now();
        validate_owner_policy(policy)
            .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Preserved))?;
        let file_synced = sync_requirement != LocalFileSyncRequirement::NotRequired && pre_synced;
        self.owner_access_policy = Some(*policy);
        if let Some(owner) = self.owner.as_mut() {
            owner.set_owner_access_policy(Some(*policy));
        }
        let (entries, traversal, maximum_entries, directory_mode_restricts_traversal) =
            self.collect_owner_policy_entries_started_at(policy, limits, started_at)?;
        if owner_policy_deadline_exceeded(limits, started_at) {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::Preserved,
            ));
        }
        let retained =
            self.open_and_pin_owner_policy_entries_started_at(&entries, sync_requirement, limits, started_at)?;
        if owner_policy_deadline_exceeded(limits, started_at) {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::Preserved,
            ));
        }
        let root = self.apply_owner_policy_mutations(policy, &retained)?;
        if owner_policy_deadline_exceeded(limits, started_at) {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::Indeterminate,
            ));
        }
        self.verify_owner_policy_entries_started_at(
            policy,
            limits,
            &entries,
            &retained,
            &traversal,
            maximum_entries,
            directory_mode_restricts_traversal,
            &root,
            started_at,
        )?;
        if owner_policy_deadline_exceeded(limits, started_at) {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::OwnerPolicyApplied,
            ));
        }
        let file_synced = self.sync_owner_policy_files(&retained, sync_requirement, file_synced, limits, started_at)?;
        Ok((
            crate::outcome::LocalTreeAccessOutcome::with_owner_policy(State::OwnerPolicyApplied, *policy),
            file_synced,
        ))
    }

    /// Collects entries for tests using the ordinary owner-policy limits.
    ///
    /// This Linux test helper exposes the private preparation phase so tests
    /// can mutate the namespace between collection and later phases.
    ///
    /// # Parameters
    ///
    /// * `policy` — policy used to determine entry ordering and traversal
    ///   needs.
    /// * `limits` — resource ceilings applied during collection.
    #[cfg(all(test, target_os = "linux"))]
    fn collect_owner_policy_entries(
        &self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
    ) -> Result<(Vec<OwnerPolicyEntry>, LocalListOptions, usize, bool), super::super::tree_recovery::LocalTreeAccessError>
    {
        self.collect_owner_policy_entries_started_at(policy, limits, std::time::Instant::now())
    }

    /// Collects and orders stage entries while enforcing one shared deadline.
    ///
    /// Entry count, encoded name bytes, traversal depth, and allocation are
    /// bounded before any access mutation begins.
    ///
    /// # Parameters
    ///
    /// * `policy` — policy whose traversal requirements are checked.
    /// * `limits` — entry, byte, depth, and deadline limits.
    /// * `started_at` — instant shared by all phases of the operation.
    ///
    /// # Returns
    ///
    /// Returns ordered observations, traversal options, the entry ceiling, and
    /// whether the chosen directory policy prevents traversal.
    ///
    /// # Errors
    ///
    /// Returns an access error if observation fails, a resource ceiling is
    /// exceeded, or a nonempty tree cannot be traversed under the policy.
    fn collect_owner_policy_entries_started_at(
        &self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
        started_at: std::time::Instant,
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
            .observe_tree_with_options_started_at(&traversal, started_at)
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

    /// Opens collected entries using default limits for phase-level tests.
    ///
    /// Tests use this helper to verify that stale names are rejected before
    /// permissions change.
    ///
    /// # Parameters
    ///
    /// * `entries` — previously observed paths, kinds, and identities.
    /// * `sync_requirement` — synchronization capability required of handles.
    #[cfg(all(test, target_os = "linux"))]
    fn open_and_pin_owner_policy_entries(
        &self,
        entries: &[OwnerPolicyEntry],
        sync_requirement: LocalFileSyncRequirement,
    ) -> Result<Vec<RetainedOwnerPolicyEntry>, super::super::tree_recovery::LocalTreeAccessError> {
        self.open_and_pin_owner_policy_entries_started_at(
            entries,
            sync_requirement,
            &LocalListOptions::new(),
            std::time::Instant::now(),
        )
    }

    /// Opens each collected entry and retains its identity-bound handle.
    ///
    /// The deadline and synchronization requirement apply to the full pinning
    /// phase; a mismatch aborts before the first mutation.
    ///
    /// # Parameters
    ///
    /// * `entries` — previously observed paths, kinds, and identities.
    /// * `sync_requirement` — required synchronization capability.
    /// * `limits` — resource and elapsed-time bounds.
    /// * `started_at` — instant shared by all phases of the operation.
    ///
    /// # Returns
    ///
    /// Returns handles pinned to each observed entry and its identity.
    ///
    /// # Errors
    ///
    /// Returns an access error if a deadline, handle, identity, or root binding
    /// check fails before mutation begins.
    fn open_and_pin_owner_policy_entries_started_at(
        &self,
        entries: &[OwnerPolicyEntry],
        sync_requirement: LocalFileSyncRequirement,
        limits: &LocalListOptions,
        started_at: std::time::Instant,
    ) -> Result<Vec<RetainedOwnerPolicyEntry>, super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(entries.len())
            .map_err(|_| super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Preserved))?;
        for (path, kind, expected) in entries {
            if owner_policy_deadline_exceeded(limits, started_at) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Preserved,
                ));
            }
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

    /// Mutates pinned entries and then the root, checking each identity
    /// binding.
    ///
    /// The retained handles prevent later path replacement from redirecting
    /// permission changes to a different entry.
    ///
    /// # Parameters
    ///
    /// * `policy` — exact owner policy to apply.
    /// * `retained` — identity-pinned handles opened during preparation.
    ///
    /// # Returns
    ///
    /// Returns a handle to the root after child and root access have been
    /// applied.
    ///
    /// # Errors
    ///
    /// Returns an access error carrying the best-known state if identity
    /// checks or a permission mutation fails.
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
    ///
    /// # Parameters
    ///
    /// * `policy` — policy whose resulting access facts must hold.
    /// * `limits` — elapsed-time, depth, and traversal resource constraints.
    /// * `entries` — original path, kind, and identity observations.
    /// * `retained` — pinned handles used for identity and permission checks.
    /// * `traversal` — bounded traversal options for the final namespace walk.
    /// * `maximum_entries` — maximum accepted final entry count.
    /// * `directory_mode_restricts_traversal` — whether the selected root mode
    ///   prevents walking an empty tree after mutation.
    /// * `root` — retained root handle whose identity and access are verified.
    ///
    /// # Returns
    ///
    /// Returns success only when the final namespace and all access facts
    /// match the expected observations; otherwise reports an access error.
    #[cfg(all(test, target_os = "linux"))]
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
        self.verify_owner_policy_entries_started_at(
            policy,
            limits,
            entries,
            retained,
            traversal,
            maximum_entries,
            directory_mode_restricts_traversal,
            root,
            std::time::Instant::now(),
        )
    }

    /// Verifies the final namespace and permissions against the shared
    /// deadline.
    ///
    /// It requires the original observations and pinned handles to distinguish
    /// replacement, addition, removal, and permission changes.
    ///
    /// # Parameters
    ///
    /// * `policy` — exact owner permissions expected after mutation.
    /// * `limits` — resource and elapsed-time bounds.
    /// * `entries` — collected namespace observations.
    /// * `retained` — handles pinned to those observations.
    /// * `traversal` — options for rewalking the final namespace.
    /// * `maximum_entries` — maximum allowed final entry count.
    /// * `directory_mode_restricts_traversal` — whether final directory access
    ///   prevents namespace traversal.
    /// * `root` — retained root handle used for final access verification.
    /// * `started_at` — instant shared by all phases of the operation.
    ///
    /// # Returns
    ///
    /// Returns success only when the final namespace, pinned entries, and root
    /// satisfy the requested policy.
    ///
    /// # Errors
    ///
    /// Returns an access error when the deadline expires or any final identity,
    /// namespace, or permission check fails.
    #[allow(clippy::too_many_arguments)]
    fn verify_owner_policy_entries_started_at(
        &mut self,
        policy: &crate::policy::LocalOwnerAccessPolicy,
        limits: &LocalListOptions,
        entries: &[OwnerPolicyEntry],
        retained: &[RetainedOwnerPolicyEntry],
        traversal: &LocalListOptions,
        maximum_entries: usize,
        directory_mode_restricts_traversal: bool,
        root: &std::fs::File,
        started_at: std::time::Instant,
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
        let mut expected: std::collections::HashMap<
            &Path,
            (crate::LocalFileKind, Option<&crate::observation::LocalFileIdentity>),
        > = std::collections::HashMap::new();
        expected.try_reserve(entries.len()).map_err(|_| {
            super::super::tree_recovery::LocalTreeAccessError::new(resource_limit(), State::Indeterminate)
        })?;
        for (path, kind, identity) in entries {
            #[cfg(test)]
            reach_owner_policy_deadline_boundary(OwnerPolicyDeadlineBoundary::IndexBuild, started_at);
            if owner_policy_deadline_exceeded(limits, started_at) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Indeterminate,
                ));
            }
            expected.insert(path.as_path(), (*kind, identity.as_ref()));
        }
        let mut final_entry_count = 0_usize;
        let mut final_name_bytes = 0_usize;
        if !entries.is_empty() || !directory_mode_restricts_traversal {
            for item in self
                .observe_tree_with_options_started_at(traversal, started_at)
                .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?
            {
                let item = item.map_err(|cause| {
                    super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate)
                })?;
                if owner_policy_deadline_exceeded(limits, started_at) {
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
                if final_entry_count >= maximum_entries {
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
                final_entry_count = final_entry_count.saturating_add(1);
                let observed = (item.observation().metadata().kind(), item.observation().identity());
                #[cfg(test)]
                OWNER_POLICY_FINAL_INDEX_QUERIES.with(|count| count.set(count.get().saturating_add(1)));
                if expected.remove(item.relative_path()) != Some(observed) {
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                        LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                        State::Indeterminate,
                    ));
                }
                #[cfg(test)]
                reach_owner_policy_deadline_boundary(OwnerPolicyDeadlineBoundary::FinalMatch, started_at);
                if owner_policy_deadline_exceeded(limits, started_at) {
                    return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                        resource_limit(),
                        State::Indeterminate,
                    ));
                }
            }
            if !expected.is_empty() {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                    State::Indeterminate,
                ));
            }
        }
        // The complete namespace set is now checked, then every pre-opened
        // identity-bound handle is checked for exact policy facts.
        for (path, kind, expected, file, _) in retained {
            #[cfg(test)]
            reach_owner_policy_deadline_boundary(OwnerPolicyDeadlineBoundary::PinnedHandle, started_at);
            if owner_policy_deadline_exceeded(limits, started_at) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::Indeterminate,
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
        if owner_policy_deadline_exceeded(limits, started_at) {
            return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                resource_limit(),
                State::Indeterminate,
            ));
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
        crate::temp::internal::privacy_windows::verify_owner_access(root, true, policy).map_err(|source| {
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
    ///
    /// Optional synchronization records incomplete syncs; required
    /// synchronization returns an error while preserving the applied state.
    ///
    /// # Parameters
    ///
    /// * `retained` — opened entries whose regular files may need syncing.
    /// * `sync_requirement` — whether synchronization is optional or required.
    /// * `file_synced` — whether earlier phases synchronized every required
    ///   file.
    /// * `limits` — elapsed-time bound for the synchronization phase.
    /// * `started_at` — instant shared by all phases of the operation.
    ///
    /// # Returns
    ///
    /// Returns whether every required file has been synchronized.
    ///
    /// # Errors
    ///
    /// Returns an access error when the deadline expires or required
    /// synchronization fails.
    fn sync_owner_policy_files(
        &self,
        retained: &[RetainedOwnerPolicyEntry],
        sync_requirement: LocalFileSyncRequirement,
        mut file_synced: bool,
        limits: &LocalListOptions,
        started_at: std::time::Instant,
    ) -> Result<bool, super::super::tree_recovery::LocalTreeAccessError> {
        use crate::outcome::LocalTreeAccessState as State;
        for (path, kind, _expected, file, sync_capable) in retained {
            if owner_policy_deadline_exceeded(limits, started_at) {
                return Err(super::super::tree_recovery::LocalTreeAccessError::new(
                    resource_limit(),
                    State::OwnerPolicyApplied,
                ));
            }
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
    /// Applies ordinary access while preserving the caller's sync requirement.
    ///
    /// This crate-internal entry point is used by publication flows that share
    /// one sync policy across multiple operations.
    ///
    /// # Parameters
    ///
    /// * `policy` — final access policy to apply.
    /// * `limits` — resource ceilings for recursive observation.
    /// * `sync_requirement` — whether changed files must be synchronized.
    ///
    /// # Returns
    ///
    /// Returns the applied access outcome and whether required files are
    /// synchronized.
    ///
    /// # Errors
    ///
    /// Returns an access error with the best-known stage state if a phase
    /// fails.
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
            let entries = self
                .collect_unix_access_entries(limits)
                .map_err(|cause| super::super::tree_recovery::LocalTreeAccessError::new(cause, State::Indeterminate))?;
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

    /// Observes and orders Unix stage entries before any access mutation.
    ///
    /// The walk keeps the ordinary policy's unbounded-depth traversal while
    /// enforcing the caller's explicit depth and encoded-name byte limits.
    /// Each returned identity is captured before the caller rechecks and
    /// opens the corresponding entry.
    ///
    /// # Parameters
    ///
    /// * `limits` — byte and depth ceilings for the observation pass.
    ///
    /// # Returns
    ///
    /// Returns paths, entry kinds, and captured identities ordered for access
    /// mutation, with non-directories before directories and deeper paths
    /// first.
    ///
    /// # Errors
    ///
    /// Returns the traversal error, a resource-limit error, or `Unsupported`
    /// when an entry identity could not be observed.
    #[cfg(unix)]
    fn collect_unix_access_entries(
        &self,
        limits: &LocalListOptions,
    ) -> LocalResult<Vec<(PathBuf, crate::LocalFileKind, crate::observation::LocalFileIdentity)>> {
        let mut entries = Vec::new();
        let mut bytes = 0_usize;
        let traversal = limits
            .without_max_depth()
            .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
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
        entries.sort_by_key(|(path, kind, _)| {
            (
                if *kind == crate::LocalFileKind::Directory { 1 } else { 0 },
                std::cmp::Reverse(path.components().count()),
            )
        });
        Ok(entries)
    }

    /// Applies the ordinary access policy through retained Windows handles.
    ///
    /// The namespace is observed before mutation and each opened handle is
    /// checked before its permissions are changed.
    ///
    /// # Parameters
    ///
    /// * `policy` — final access policy to apply.
    /// * `limits` — resource ceilings for recursive observation.
    /// * `sync_requirement` — whether changed files must be synchronized.
    ///
    /// # Returns
    ///
    /// Returns the applied Windows access outcome and sync status.
    ///
    /// # Errors
    ///
    /// Returns an access error if collection, mutation, verification, or sync
    /// fails.
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
    use super::OWNER_POLICY_FINAL_INDEX_QUERIES;
    use super::OWNER_POLICY_TEST_DEADLINE_BOUNDARY;
    use super::OWNER_POLICY_TEST_NOW;
    use super::OwnerPolicyDeadlineBoundary;
    use super::OwnerPolicySyncEvent;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalListOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::outcome::LocalTreeAccessState;
    use crate::policy::LocalFileSyncRequirement;
    use crate::policy::LocalOwnerAccessPolicy;

    struct DeadlineBoundaryGuard;

    impl DeadlineBoundaryGuard {
        fn expire_at(boundary: OwnerPolicyDeadlineBoundary) -> Self {
            OWNER_POLICY_TEST_NOW.with(|now| now.set(None));
            OWNER_POLICY_TEST_DEADLINE_BOUNDARY.with(|selected| selected.set(Some(boundary)));
            Self
        }
    }

    impl Drop for DeadlineBoundaryGuard {
        fn drop(&mut self) {
            OWNER_POLICY_TEST_DEADLINE_BOUNDARY.with(|selected| selected.set(None));
            OWNER_POLICY_TEST_NOW.with(|now| now.set(None));
        }
    }

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
    fn test_final_verification_accepts_the_same_set_in_a_different_order() {
        let (_scratch, mut stage, policy) = fixture(true);
        stage
            .write_file_new(Path::new("second"), &mut &b"second"[..], 16)
            .expect("second file");
        let (mut entries, traversal, maximum_entries, restricted) = stage
            .collect_owner_policy_entries(&policy, &LocalListOptions::new())
            .expect("collect identities");
        let retained = stage
            .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
            .expect("pin handles");
        let root = stage
            .apply_owner_policy_mutations(&policy, &retained)
            .expect("apply policy");
        entries.reverse();
        stage
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
            .expect("ordering does not affect exact-set verification");
        let _recovery = stage.retain_in_place();
    }

    #[test]
    fn test_final_verification_queries_one_index_entry_per_observation() {
        for count in [16, 256, 512] {
            let (_scratch, mut stage, policy) = fixture(false);
            for index in 0..count {
                let relative = format!("entry-{index:04}");
                stage
                    .write_file_new(Path::new(&relative), &mut &b"x"[..], 1)
                    .expect("entry");
            }
            let (entries, traversal, maximum_entries, restricted) = stage
                .collect_owner_policy_entries(&policy, &LocalListOptions::new().with_max_entries(count))
                .expect("collect entries");
            let retained = stage
                .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
                .expect("pin entries");
            let root = stage
                .apply_owner_policy_mutations(&policy, &retained)
                .expect("apply policy");
            OWNER_POLICY_FINAL_INDEX_QUERIES.with(|queries| queries.set(0));
            stage
                .verify_owner_policy_entries(
                    &policy,
                    &LocalListOptions::new().with_max_entries(count),
                    &entries,
                    &retained,
                    &traversal,
                    maximum_entries,
                    restricted,
                    &root,
                )
                .expect("final set should match");
            OWNER_POLICY_FINAL_INDEX_QUERIES.with(|queries| assert_eq!(count, queries.get()));
            let _recovery = stage.retain_in_place();
        }
    }

    #[test]
    fn test_final_verification_rejects_an_expired_absolute_deadline() {
        let (_scratch, mut stage, policy) = fixture(true);
        let limits = LocalListOptions::new().with_deadline(std::time::Duration::from_secs(1));
        let (entries, traversal, maximum_entries, restricted) = stage
            .collect_owner_policy_entries(&policy, &limits)
            .expect("collect identities");
        let retained = stage
            .open_and_pin_owner_policy_entries(&entries, LocalFileSyncRequirement::NotRequired)
            .expect("pin handles");
        let root = stage
            .apply_owner_policy_mutations(&policy, &retained)
            .expect("apply policy");
        let started_at = std::time::Instant::now() - std::time::Duration::from_secs(2);
        let error = stage
            .verify_owner_policy_entries_started_at(
                &policy,
                &limits,
                &entries,
                &retained,
                &traversal,
                maximum_entries,
                restricted,
                &root,
                started_at,
            )
            .expect_err("final verification must use the original elapsed budget");
        assert_eq!(LocalFileErrorKind::ResourceLimit, error.cause().kind());
        assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state());
        let _recovery = stage.retain_in_place();
    }

    #[test]
    fn test_owner_policy_deadline_stops_final_verification_at_each_boundary() {
        for boundary in [
            OwnerPolicyDeadlineBoundary::IndexBuild,
            OwnerPolicyDeadlineBoundary::FinalMatch,
            OwnerPolicyDeadlineBoundary::PinnedHandle,
        ] {
            let (_scratch, mut stage, policy) = fixture(true);
            let _clock = DeadlineBoundaryGuard::expire_at(boundary);
            super::super::sync::OWNER_POLICY_SYNC_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
            let limits = LocalListOptions::new().with_deadline(std::time::Duration::from_secs(30));
            let error = stage
                .apply_owner_access_policy_with_sync(&policy, &limits, LocalFileSyncRequirement::Required, false)
                .expect_err("deadline expiration prevents the next owner-policy phase");
            assert_eq!(LocalFileErrorKind::ResourceLimit, error.cause().kind());
            assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state());
            let events =
                super::super::sync::OWNER_POLICY_SYNC_EVENTS.with(|events| events.borrow_mut().take().unwrap());
            assert!(
                events
                    .iter()
                    .all(|event| !matches!(event, OwnerPolicySyncEvent::FinalSync(_))),
                "expired verification must not start the final sync"
            );
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
            let result = stage.sync_owner_policy_files(
                &retained,
                requirement,
                true,
                &LocalListOptions::new(),
                std::time::Instant::now(),
            );
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
                    .sync_owner_policy_files(
                        &retained,
                        requirement,
                        true,
                        &LocalListOptions::new(),
                        std::time::Instant::now(),
                    )
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
                "collect" | "pin" => {
                    assert_eq!(LocalTreeAccessState::Preserved, error.access_state())
                }
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
                "collect" | "pin" => {
                    assert_eq!(0o644, mode, "non-mutating phase preserves file mode")
                }
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
}
