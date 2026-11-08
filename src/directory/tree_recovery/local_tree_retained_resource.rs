// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use super::local_tree_cleanup_error::LocalTreeCleanupError;
use super::local_tree_recovery_location::LocalTreeRecoveryLocation;
use crate::directory::LocalDirectoryHandle;
use crate::observation::LocalFileIdentity;
use crate::options::LocalDeleteOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempCleanupLimits;
use crate::outcome::LocalTreeMutationState;

#[cfg(test)]
thread_local! {
    static CLEANUP_INDEX_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CLEANUP_TEST_HOOK: std::cell::RefCell<Option<CleanupTestHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[derive(Default)]
struct CleanupTestHook {
    path: PathBuf,
    calls: usize,
    expire_after_descendant: bool,
    expire_after_root_identity: bool,
    started_at: Option<std::time::Instant>,
    now: Option<std::time::Instant>,
    root_observations: usize,
    root_deletions: usize,
}

/// An identity-bound tree resource retained after an operation failure.
///
/// The owner keeps the authority and observed identity needed for explicit
/// cleanup. Dropping an armed owner makes a final best-effort cleanup attempt;
/// call [`Self::retain_in_place`] to relinquish that cleanup responsibility.
/// [`Self::diagnostic_path`] is a namespace coordinate for reporting and does
/// not itself preserve authority or bind later path operations to this entry.
///
/// # Examples
///
/// This helper reports the diagnostic coordinate without consuming the owner;
/// the caller can still retry cleanup afterwards.
///
/// ```
/// use qubit_local_files::directory::LocalTreeRetainedResource;
/// use std::path::Path;
///
/// fn diagnostic_path(resource: &LocalTreeRetainedResource) -> &Path {
///     resource.diagnostic_path()
/// }
/// ```
pub struct LocalTreeRetainedResource {
    /// Diagnostic namespace path; cleanup uses the retained parent and relative
    /// name.
    path: PathBuf,
    /// Child name addressed through the retained parent authority.
    relative: PathBuf,
    /// Captured identity; cleanup refuses to remove a name when this is
    /// unknown.
    identity: Option<LocalFileIdentity>,
    /// Retained parent authority used to bind cleanup to the original
    /// namespace.
    parent: LocalDirectoryHandle,
    /// Open child authority, when available for safe recursive cleanup.
    child: Option<LocalDirectoryHandle>,
    /// Whether Drop may attempt cleanup; reopened owners stay disarmed until
    /// validated.
    cleanup_armed: bool,
    /// Resource limits applied to explicit cleanup and Drop's final attempt.
    cleanup_limits: LocalTempCleanupLimits,
    /// Identities observed during creation, checked before owned names are
    /// removed.
    owned_entries: HashMap<PathBuf, LocalFileIdentity>,
    /// Whether cleanup has begun mutating the namespace.
    mutation_state: LocalTreeMutationState,
    /// Owner access policy that must remain attached to this recovery owner.
    owner_access_policy: Option<crate::policy::LocalOwnerAccessPolicy>,
}

impl std::fmt::Debug for LocalTreeRetainedResource {
    /// Formats the owner while redacting namespace paths.
    ///
    /// # Parameters
    ///
    /// - `formatter`: Destination for the debug representation.
    ///
    /// # Returns
    ///
    /// Returns the formatter's result.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalTreeRetainedResource")
            .field("path", &crate::local_file_system::redacted_path(&self.path))
            .field("relative", &crate::local_file_system::redacted_path(&self.relative))
            .field("identity", &self.identity)
            .field("parent", &self.parent)
            .field("child", &self.child)
            .field("cleanup_armed", &self.cleanup_armed)
            .field("cleanup_limits", &self.cleanup_limits)
            .field("owned_entries", &self.owned_entries.len())
            .field("mutation_state", &self.mutation_state)
            .field("owner_access_policy", &self.owner_access_policy)
            .finish()
    }
}

impl LocalTreeRetainedResource {
    /// Creates a reopened owner disarmed until its parent binding is
    /// revalidated.
    ///
    /// # Parameters
    ///
    /// - `path`: Diagnostic namespace path for the resource.
    /// - `relative`: Child name addressed through `parent`.
    /// - `identity`: Captured child identity, when available.
    /// - `parent`: Retained authority for the containing directory.
    /// - `child`: Optional authority for recursive cleanup.
    /// - `cleanup_limits`: Limits applied to cleanup attempts.
    ///
    /// # Returns
    ///
    /// Returns an owner that remains disarmed until its binding is validated.
    pub(crate) fn new_for_reopen(
        path: PathBuf,
        relative: PathBuf,
        identity: Option<LocalFileIdentity>,
        parent: LocalDirectoryHandle,
        child: Option<LocalDirectoryHandle>,
        cleanup_limits: LocalTempCleanupLimits,
    ) -> Self {
        let mut resource = Self::new(path, relative, identity, parent, child, cleanup_limits);
        resource.cleanup_armed = false;
        resource
    }

    /// Creates an armed owner bound to a retained parent and optional child
    /// handle.
    ///
    /// # Parameters
    ///
    /// - `path`: Diagnostic namespace path for the resource.
    /// - `relative`: Child name addressed through `parent`.
    /// - `identity`: Captured child identity, when available.
    /// - `parent`: Retained authority for the containing directory.
    /// - `child`: Optional authority for recursive cleanup.
    /// - `cleanup_limits`: Limits applied to cleanup attempts.
    ///
    /// # Returns
    ///
    /// Returns an armed owner that attempts best-effort cleanup when dropped.
    pub(crate) fn new(
        path: PathBuf,
        relative: PathBuf,
        identity: Option<LocalFileIdentity>,
        parent: LocalDirectoryHandle,
        child: Option<LocalDirectoryHandle>,
        cleanup_limits: LocalTempCleanupLimits,
    ) -> Self {
        Self {
            path,
            relative,
            identity,
            parent,
            child,
            cleanup_armed: true,
            cleanup_limits,
            owned_entries: HashMap::new(),
            mutation_state: LocalTreeMutationState::NotStarted,
            owner_access_policy: None,
        }
    }

    /// Returns the namespace path kept for diagnostics.
    ///
    /// This path is not an authority and does not guarantee that resolving it
    /// later reaches the identity-bound resource.
    ///
    /// # Returns
    ///
    /// Returns the stored diagnostic path; it does not grant filesystem access.
    #[must_use]
    pub fn diagnostic_path(&self) -> &Path {
        &self.path
    }
    /// Returns the captured native identity, when available.
    ///
    /// # Returns
    ///
    /// Returns `Some` when a native identity was captured, or `None` when it
    /// was unavailable.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Returns the owner policy retained for this recovery resource.
    ///
    /// # Returns
    ///
    /// Returns `Some` when an owner access policy is attached, or `None`
    /// otherwise.
    pub const fn owner_access_policy(&self) -> Option<&crate::policy::LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }

    /// Sets limits for explicit cleanup and the final Drop attempt.
    ///
    /// Dropping this armed owner runs only a best-effort attempt with these
    /// limits; use [`Self::cleanup`] to observe failures and retain the owner
    /// for retry.
    ///
    /// # Parameters
    ///
    /// - `limits`: Limits to use for explicit cleanup and Drop cleanup.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        self.cleanup_limits = limits;
    }
    /// Removes the retained tree and disarms this owner when cleanup succeeds.
    ///
    /// On failure, returns an error that still owns this resource so cleanup
    /// can be retried or the owner can be transferred elsewhere.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when cleanup succeeds. On failure, returns the cause
    /// together with ownership of this resource.
    pub fn cleanup(mut self) -> Result<(), LocalTreeCleanupError> {
        match self.cleanup_inner() {
            Ok(()) => {
                self.cleanup_armed = false;
                Ok(())
            }
            Err(cause) => Err(LocalTreeCleanupError::new(cause, self.mutation_state, self)),
        }
    }
    /// Disarms automatic cleanup and returns the recorded namespace path and
    /// identity.
    ///
    /// The returned location is diagnostic metadata, not a cleanup authority.
    /// The consumed owner is disarmed, so this relinquishes its automatic
    /// cleanup responsibility.
    ///
    /// # Returns
    ///
    /// Returns the recorded namespace path and optional identity without
    /// retaining cleanup authority.
    #[must_use = "the recovery location must be retained to preserve its diagnostic metadata"]
    pub fn retain_in_place(self) -> LocalTreeRecoveryLocation {
        let mut this = self;
        this.cleanup_armed = false;
        LocalTreeRecoveryLocation::new(std::mem::take(&mut this.path), this.identity.take())
    }

    /// Returns a file handle for the child through the retained parent
    /// authority on Windows.
    ///
    /// The diagnostic path is used only in errors; opening is relative to the
    /// retained parent and child name.
    ///
    /// # Returns
    ///
    /// Returns the opened child file, or an error if authority cloning or
    /// opening fails.
    ///
    /// # Errors
    ///
    /// Returns an error when the parent authority cannot be cloned or the
    /// child cannot be opened.
    #[cfg(windows)]
    pub(crate) fn open_child_for_access(&self) -> crate::LocalResult<std::fs::File> {
        let parent = self.parent.root_authority()?.try_clone_authority().map_err(|source| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenRoot,
                Some(self.path.clone()),
                None,
                source,
            )
        })?;
        crate::temp::internal::privacy_windows::open_existing_relative(&parent, &self.relative, true).map_err(
            |source| {
                crate::LocalFileError::from_io(
                    crate::LocalFileOperation::OpenRoot,
                    Some(self.path.clone()),
                    None,
                    source,
                )
            },
        )
    }

    /// Transfers namespace ownership to publication and disables Drop cleanup.
    ///
    /// The child handle is released because publication now owns the
    /// namespace entry.
    pub(crate) fn disarm_after_publish(&mut self) {
        self.cleanup_armed = false;
        self.child.take();
    }
    /// Checks whether `parent` and the retained parent refer to the same
    /// object.
    ///
    /// # Parameters
    ///
    /// - `parent`: Directory handle to compare with the retained parent.
    ///
    /// # Returns
    ///
    /// Returns whether both handles report the same available identity.
    ///
    /// # Errors
    ///
    /// Returns an error if either handle cannot be observed.
    pub(crate) fn parent_matches(&self, parent: &LocalDirectoryHandle) -> crate::LocalResult<bool> {
        let expected = self.parent.observe()?.identity().cloned();
        let actual = parent.observe()?.identity().cloned();
        Ok(expected.is_some() && expected == actual)
    }
    /// Checks that the retained parent still names the captured child identity.
    ///
    /// # Returns
    ///
    /// Returns whether the named child still has the captured identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the retained parent cannot observe the child.
    pub(crate) fn binding_matches_identity(&self) -> crate::LocalResult<bool> {
        let named = self.parent.observe_entry(&self.relative)?;
        Ok(self
            .identity
            .as_ref()
            .is_some_and(|identity| named.identity() == Some(identity)))
    }
    /// Records one identity owned by this tree for a later guarded deletion.
    ///
    /// # Parameters
    ///
    /// - `relative`: Child path relative to the retained tree root.
    /// - `identity`: Identity observed for that child.
    ///
    /// # Errors
    ///
    /// Returns `InvalidState` if a previously recorded path has a different
    /// identity, or `ResourceLimit` if the ownership index cannot grow.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when the identity is recorded or already matches the
    /// recorded identity.
    pub(crate) fn record_entry(&mut self, relative: &Path, identity: &LocalFileIdentity) -> crate::LocalResult<()> {
        if let Some(recorded_identity) = self.owned_entries.get(relative) {
            if recorded_identity == identity {
                return Ok(());
            }
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::CreateTempFile,
            )
            .with_path(self.path.join(relative)));
        }
        self.owned_entries.try_reserve(1).map_err(|_| {
            crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                crate::LocalFileOperation::CreateTempFile,
            )
        })?;
        self.owned_entries.insert(relative.to_path_buf(), identity.clone());
        Ok(())
    }
    /// Arms best-effort Drop cleanup after the reopened name matched its
    /// identity.
    pub(crate) fn arm_after_reopen_validation(&mut self) {
        self.cleanup_armed = true;
    }

    /// Replaces the policy retained for later cleanup and recovery.
    ///
    /// # Parameters
    ///
    /// - `policy`: Exact owner access policy to retain, if configured.
    pub(crate) fn set_owner_access_policy(&mut self, policy: Option<crate::policy::LocalOwnerAccessPolicy>) {
        self.owner_access_policy = policy;
    }

    /// Returns this owner with the exact access policy attached to it.
    ///
    /// # Parameters
    ///
    /// - `policy`: Exact owner access policy to retain, if configured.
    ///
    /// # Returns
    ///
    /// Returns this owner with `policy` attached.
    #[must_use = "the updated owner must be retained for the policy to take effect"]
    pub(crate) fn with_owner_access_policy(mut self, policy: Option<crate::policy::LocalOwnerAccessPolicy>) -> Self {
        self.owner_access_policy = policy;
        self
    }

    #[cfg(test)]
    pub(crate) fn reset_cleanup_index_queries() {
        CLEANUP_INDEX_QUERIES.with(|queries| queries.set(0));
    }

    #[cfg(test)]
    pub(crate) fn cleanup_index_queries() -> usize {
        CLEANUP_INDEX_QUERIES.with(std::cell::Cell::get)
    }
}

impl LocalTreeRetainedResource {
    /// Attempts identity-checked cleanup without disarming ownership on
    /// failure.
    ///
    /// # Errors
    ///
    /// Returns an error when the recorded name or an owned descendant no longer
    /// matches its identity, when a cleanup budget is exceeded, or when native
    /// observation or deletion fails.
    ///
    /// # Parameters
    ///
    /// - `self`: Mutable owner whose retained tree is cleaned up.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` after removing the retained tree.
    fn cleanup_inner(&mut self) -> crate::LocalResult<()> {
        #[cfg(test)]
        CLEANUP_TEST_HOOK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut()
                && hook.path == self.path
            {
                hook.calls = hook.calls.saturating_add(1);
            }
        });
        if !self.cleanup_armed {
            return Ok(());
        }
        let started_at = std::time::Instant::now();
        if self.cleanup_limits.max_entries() == Some(0) || cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                crate::LocalFileOperation::Cleanup,
            ));
        }
        let mut pending_path_bytes = self.path.as_os_str().as_encoded_bytes().len();
        if self
            .cleanup_limits
            .max_pending_path_bytes()
            .is_some_and(|maximum| pending_path_bytes > maximum)
        {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                crate::LocalFileOperation::Cleanup,
            )
            .with_path(self.path.clone()));
        }
        let named = self.parent.observe_entry(&self.relative)?;
        if self
            .identity
            .as_ref()
            .is_none_or(|identity| named.identity() != Some(identity))
        {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Cleanup,
            )
            .with_path(self.path.clone()));
        }
        if let Some(child) = self.child.as_ref() {
            let mut entries = Vec::new();
            let mut list_options = LocalListOptions::new()
                .with_recursive()
                .with_symlink_policy(crate::LocalSymlinkPolicy::Reject);
            if let Some(maximum) = self.cleanup_limits.max_entries() {
                list_options = list_options.with_max_entries(maximum.saturating_sub(1));
            }
            if let Some(maximum) = self.cleanup_limits.max_pending_path_bytes() {
                list_options = list_options.with_max_seen_name_bytes(maximum);
            }
            if let Some(deadline) = self.cleanup_limits.deadline() {
                list_options = list_options.with_deadline(deadline);
            }
            for entry in child.list_with_options(Path::new("."), &list_options)? {
                if cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    ));
                }
                let entry = entry?;
                let relative = entry.relative_path().to_path_buf();
                let depth = relative.components().count();
                if let Some(maximum) = self.cleanup_limits.max_depth()
                    && depth > maximum
                {
                    return Err(crate::LocalFileError::from_resource_limit(
                        crate::LocalFileOperation::Cleanup,
                        Some(self.path.join(&relative)),
                        crate::LocalResourceLimitError::new(crate::LocalResourceKind::Depth, maximum, 0, depth)
                            .with_observed(depth),
                    ));
                }
                pending_path_bytes = pending_path_bytes
                    .checked_add(relative.as_os_str().as_encoded_bytes().len())
                    .ok_or_else(|| {
                        crate::LocalFileError::new(
                            crate::LocalFileErrorKind::ResourceLimit,
                            crate::LocalFileOperation::Cleanup,
                        )
                    })?;
                if self
                    .cleanup_limits
                    .max_pending_path_bytes()
                    .is_some_and(|maximum| pending_path_bytes > maximum)
                {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    ));
                }
                let observation = child.observe_entry(&relative)?;
                let identity = observation.identity().cloned().ok_or_else(|| {
                    crate::LocalFileError::new(
                        crate::LocalFileErrorKind::Unsupported,
                        crate::LocalFileOperation::Cleanup,
                    )
                    .with_path(self.path.join(&relative))
                })?;
                entries.try_reserve(1).map_err(|_| {
                    crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    )
                })?;
                entries.push((relative, observation.metadata().kind(), identity));
            }
            entries.sort_by_key(|(path, _, _)| std::cmp::Reverse(path.components().count()));
            for (relative, kind, expected) in entries {
                if cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    ));
                }
                let current = child.observe_entry(&relative)?;
                if current.identity() != Some(&expected) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::InvalidState,
                        crate::LocalFileOperation::Cleanup,
                    )
                    .with_path(self.path.join(&relative)));
                }
                #[cfg(test)]
                CLEANUP_INDEX_QUERIES.with(|queries| queries.set(queries.get().saturating_add(1)));
                if self
                    .owned_entries
                    .get(relative.as_path())
                    .is_some_and(|owned_identity| current.identity() != Some(owned_identity))
                {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::InvalidState,
                        crate::LocalFileOperation::Cleanup,
                    )
                    .with_path(self.path.join(&relative)));
                }
                let result = if kind == crate::LocalFileKind::Directory {
                    child.filesystem().delete_directory(&relative).map(|_| ())
                } else {
                    child.filesystem().delete_file(&relative).map(|_| ())
                };
                self.mutation_state = LocalTreeMutationState::Started;
                result?;
                #[cfg(test)]
                CLEANUP_TEST_HOOK.with(|hook| {
                    if let Some(hook) = hook.borrow_mut().as_mut()
                        && hook.path == self.path
                        && hook.expire_after_descendant
                    {
                        advance_cleanup_test_clock(hook, started_at, self.cleanup_limits.deadline());
                    }
                });
            }
            drop(self.child.take());
        }
        if cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                crate::LocalFileOperation::Cleanup,
            )
            .with_path(self.path.clone()));
        }
        #[cfg(test)]
        CLEANUP_TEST_HOOK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut()
                && hook.path == self.path
            {
                hook.root_observations = hook.root_observations.saturating_add(1);
            }
        });
        match self.parent.observe_entry(&self.relative) {
            Ok(observation)
                if self
                    .identity
                    .as_ref()
                    .is_some_and(|identity| observation.identity() == Some(identity)) =>
            {
                #[cfg(test)]
                CLEANUP_TEST_HOOK.with(|hook| {
                    if let Some(hook) = hook.borrow_mut().as_mut()
                        && hook.path == self.path
                        && hook.expire_after_root_identity
                    {
                        advance_cleanup_test_clock(hook, started_at, self.cleanup_limits.deadline());
                    }
                });
                if cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    )
                    .with_path(self.path.clone()));
                }
                #[cfg(test)]
                CLEANUP_TEST_HOOK.with(|hook| {
                    if let Some(hook) = hook.borrow_mut().as_mut()
                        && hook.path == self.path
                    {
                        hook.root_deletions = hook.root_deletions.saturating_add(1);
                    }
                });
                self.mutation_state = LocalTreeMutationState::Started;
                match self
                    .parent
                    .filesystem()
                    .delete_directory_with_options(&self.relative, &LocalDeleteOptions::new())
                {
                    Ok(_) => Ok(()),
                    Err(error) => {
                        self.child = self
                            .parent
                            .open_directory(&self.relative, &crate::options::LocalDirectoryOpenOptions::new())
                            .ok();
                        Err(error)
                    }
                }
            }
            Ok(_) => Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Cleanup,
            )
            .with_path(self.path.clone())),
            Err(cause) => Err(cause),
        }
    }
}

/// Reports whether one cleanup attempt has reached its cooperative deadline.
///
/// # Parameters
///
/// - `limits`: Cleanup policy containing the optional deadline.
/// - `started_at`: Instant when the current attempt began.
///
/// # Returns
///
/// Returns `true` once the configured duration has elapsed; no deadline means
/// the function returns `false`.
#[must_use]
fn cleanup_deadline_exceeded(limits: LocalTempCleanupLimits, started_at: std::time::Instant) -> bool {
    #[cfg(test)]
    let now = CLEANUP_TEST_HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        let Some(hook) = hook.as_mut() else {
            return std::time::Instant::now();
        };
        hook.started_at.get_or_insert(started_at);
        hook.now.unwrap_or_else(std::time::Instant::now)
    });
    #[cfg(not(test))]
    let now = std::time::Instant::now();
    limits
        .deadline()
        .is_some_and(|deadline| now.duration_since(started_at) >= deadline)
}

#[cfg(test)]
fn advance_cleanup_test_clock(
    hook: &mut CleanupTestHook,
    started_at: std::time::Instant,
    deadline: Option<std::time::Duration>,
) {
    let started_at = *hook.started_at.get_or_insert(started_at);
    hook.now = deadline.and_then(|deadline| started_at.checked_add(deadline + std::time::Duration::from_nanos(1)));
}

impl Drop for LocalTreeRetainedResource {
    /// Makes one best-effort cleanup attempt while this owner remains armed.
    ///
    /// Cleanup errors cannot be returned from `Drop`; callers that need to
    /// observe or retry failures should call
    /// [`LocalTreeRetainedResource::cleanup`].
    fn drop(&mut self) {
        // Drop cannot report cleanup errors; explicit cleanup preserves them for retry.
        if self.cleanup_armed {
            let _ = self.cleanup_inner();
        }
    }
}

#[cfg(test)]
mod retained_cleanup_tests {
    use std::path::Path;

    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempCleanupLimits;
    use crate::options::LocalTempDirectoryOptions;

    struct CleanupHookGuard;

    impl CleanupHookGuard {
        fn install(path: &Path, expire_after_descendant: bool) -> Self {
            super::CLEANUP_TEST_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(super::CleanupTestHook {
                    path: path.to_path_buf(),
                    expire_after_descendant,
                    ..Default::default()
                });
            });
            Self
        }

        fn snapshot(&self) -> (usize, usize, usize, bool) {
            super::CLEANUP_TEST_HOOK.with(|hook| {
                let hook = hook.borrow();
                let hook = hook.as_ref().expect("cleanup hook installed");
                (
                    hook.calls,
                    hook.root_observations,
                    hook.root_deletions,
                    hook.now.is_some(),
                )
            })
        }

        fn expire_after_root_identity(path: &Path) -> Self {
            super::CLEANUP_TEST_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(super::CleanupTestHook {
                    path: path.to_path_buf(),
                    expire_after_root_identity: true,
                    ..Default::default()
                });
            });
            Self
        }
    }

    impl Drop for CleanupHookGuard {
        fn drop(&mut self) {
            super::CLEANUP_TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        }
    }

    fn stage_with_child() -> (
        tempfile::TempDir,
        crate::directory::LocalStagedDirectory,
        std::path::PathBuf,
    ) {
        let scratch = tempfile::tempdir().expect("scratch");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        stage
            .write_file_new(Path::new("child"), &mut &b"x"[..], 8)
            .expect("child file");
        let native = scratch.path().join(stage.relative_name());
        #[cfg(target_os = "macos")]
        let native = std::fs::canonicalize(native).expect("canonicalize staged directory path");
        (scratch, stage, native)
    }

    #[test]
    fn test_stage_drop_attempts_cleanup_once() {
        let (_scratch, mut stage, native) = stage_with_child();
        stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(0));
        let hook = CleanupHookGuard::install(&native, false);
        drop(stage);
        assert_eq!((1, 0, 0, false), hook.snapshot());
        assert!(native.exists());
    }

    #[test]
    fn test_stage_drop_successful_cleanup_has_no_extra_attempt() {
        let (_scratch, stage, native) = stage_with_child();
        let hook = CleanupHookGuard::install(&native, false);
        stage.cleanup().expect("cleanup succeeds");
        assert_eq!(1, hook.snapshot().0);
        assert!(!native.exists());
    }

    #[test]
    fn test_stage_drop_explicit_cleanup_failure_then_error_drop_attempts_twice() {
        let (_scratch, mut stage, native) = stage_with_child();
        stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(0));
        let hook = CleanupHookGuard::install(&native, false);
        let error = stage.cleanup().expect_err("depth zero cleanup");
        assert_eq!(1, hook.snapshot().0);
        drop(error);
        assert_eq!(2, hook.snapshot().0);
        assert!(native.exists());
    }

    #[test]
    fn test_stage_drop_disarmed_owner_does_not_attempt_cleanup() {
        let (_scratch, mut stage, native) = stage_with_child();
        let owner = stage.owner.take().expect("owner");
        let hook = CleanupHookGuard::install(&native, false);
        let _location = owner.retain_in_place();
        drop(stage);
        assert_eq!(0, hook.snapshot().0);
        assert!(native.exists());
    }

    #[test]
    fn test_cleanup_final_root_deadline_stops_after_last_descendant() {
        let (_scratch, mut stage, native) = stage_with_child();
        stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_deadline(std::time::Duration::from_secs(60)));
        let hook = CleanupHookGuard::install(&native, true);
        let result = stage.cleanup();
        assert_eq!(
            (1, 0, 0, true),
            hook.snapshot(),
            "root observe/delete counters prove deadline stopped cleanup"
        );
        let error = result.expect_err("deadline after descendant deletion");
        assert_eq!(crate::outcome::LocalTreeMutationState::Started, error.mutation_state());
        let (cause, mut owners) = error.into_parts();
        assert_eq!(crate::LocalFileErrorKind::ResourceLimit, cause.kind());
        assert_eq!(crate::LocalFileOperation::Cleanup, cause.operation());
        assert_eq!(Some(native.as_path()), cause.path());
        assert!(!native.join("child").exists());
        assert!(native.exists());
        drop(hook);
        for owner in &mut owners {
            owner.set_cleanup_limits(LocalTempCleanupLimits::new().with_deadline(std::time::Duration::from_secs(60)));
        }
        for owner in owners {
            owner.cleanup().expect("retry empty root after clock restoration");
        }
        assert!(!native.exists());
    }

    #[test]
    fn test_cleanup_final_root_deadline_stops_before_root_delete() {
        let (_scratch, mut stage, native) = stage_with_child();
        stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_deadline(std::time::Duration::from_secs(60)));
        let hook = CleanupHookGuard::expire_after_root_identity(&native);
        let error = stage.cleanup().expect_err("deadline after root identity observation");
        assert_eq!((1, 1, 0, true), hook.snapshot());
        assert_eq!(crate::outcome::LocalTreeMutationState::Started, error.mutation_state());
        let (cause, owners) = error.into_parts();
        assert_eq!(crate::LocalFileErrorKind::ResourceLimit, cause.kind());
        assert_eq!(crate::LocalFileOperation::Cleanup, cause.operation());
        assert_eq!(Some(native.as_path()), cause.path());
        assert!(!native.join("child").exists());
        assert!(native.exists());
        drop(hook);
        for owner in owners {
            owner.cleanup().expect("retry empty root after clock restoration");
        }
        assert!(!native.exists());
    }

    #[test]
    fn test_empty_tree_root_budget_is_checked_without_child_handle() {
        let scratch = tempfile::tempdir().expect("scratch");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        let native = scratch.path().join(stage.relative_name());
        let owner = stage.owner.as_mut().expect("retained owner");
        owner.child.take();
        owner.cleanup_limits = LocalTempCleanupLimits::new().with_max_pending_path_bytes(0);
        let error = owner
            .cleanup_inner()
            .expect_err("root path must fit even after child handle is released");
        assert_eq!(crate::LocalFileErrorKind::ResourceLimit, error.kind());
        assert_eq!(crate::LocalFileOperation::Cleanup, error.operation());
        assert_eq!(Some(owner.path.as_path()), error.path());
        assert!(native.exists());
    }
}
