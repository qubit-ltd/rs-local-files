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

#[cfg(test)]
thread_local! {
    static CLEANUP_INDEX_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

use super::local_tree_cleanup_error::LocalTreeCleanupError;
use super::local_tree_recovery_location::LocalTreeRecoveryLocation;
use crate::directory::LocalDirectoryHandle;
use crate::observation::LocalFileIdentity;
use crate::options::LocalDeleteOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempCleanupLimits;
use crate::outcome::LocalTreeMutationState;

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
    // `path` is diagnostic only; all cleanup operations use the retained parent and relative name.
    path: PathBuf,
    relative: PathBuf,
    // Cleanup refuses to remove a name when its captured identity is unknown.
    identity: Option<LocalFileIdentity>,
    parent: LocalDirectoryHandle,
    child: Option<LocalDirectoryHandle>,
    // Reopen owners remain disarmed until validation confirms the recorded binding.
    cleanup_armed: bool,
    cleanup_limits: LocalTempCleanupLimits,
    // Identities recorded during creation are compared before their names are removed.
    owned_entries: HashMap<PathBuf, LocalFileIdentity>,
    mutation_state: LocalTreeMutationState,
    owner_access_policy: Option<crate::policy::LocalOwnerAccessPolicy>,
}

impl std::fmt::Debug for LocalTreeRetainedResource {
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
    #[cfg(test)]
    pub(crate) fn reset_cleanup_index_queries() {
        CLEANUP_INDEX_QUERIES.with(|queries| queries.set(0));
    }

    #[cfg(test)]
    pub(crate) fn cleanup_index_queries() -> usize {
        CLEANUP_INDEX_QUERIES.with(std::cell::Cell::get)
    }

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

    pub(crate) fn arm_after_reopen_validation(&mut self) {
        self.cleanup_armed = true;
    }

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

    pub(crate) fn set_owner_access_policy(&mut self, policy: Option<crate::policy::LocalOwnerAccessPolicy>) {
        self.owner_access_policy = policy;
    }

    pub(crate) fn with_owner_access_policy(mut self, policy: Option<crate::policy::LocalOwnerAccessPolicy>) -> Self {
        self.owner_access_policy = policy;
        self
    }

    /// Returns the owner policy retained for this recovery resource.
    pub const fn owner_access_policy(&self) -> Option<&crate::policy::LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
    /// Returns the namespace path kept for diagnostics.
    ///
    /// This path is not an authority and does not guarantee that resolving it
    /// later reaches the identity-bound resource.
    pub fn diagnostic_path(&self) -> &Path {
        &self.path
    }
    /// Returns the captured native identity, when available.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Sets limits for explicit cleanup and the final Drop attempt.
    ///
    /// Dropping this armed owner runs only a best-effort attempt with these
    /// limits; use [`Self::cleanup`] to observe failures and retain the owner
    /// for retry.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        self.cleanup_limits = limits;
    }
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

    pub(crate) fn disarm_after_publish(&mut self) {
        self.cleanup_armed = false;
        self.child.take();
    }
    pub(crate) fn parent_matches(&self, parent: &LocalDirectoryHandle) -> crate::LocalResult<bool> {
        let expected = self.parent.observe()?.identity().cloned();
        let actual = parent.observe()?.identity().cloned();
        Ok(expected.is_some() && expected == actual)
    }
    pub(crate) fn binding_matches_identity(&self) -> crate::LocalResult<bool> {
        let named = self.parent.observe_entry(&self.relative)?;
        Ok(self
            .identity
            .as_ref()
            .is_some_and(|identity| named.identity() == Some(identity)))
    }
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
    fn cleanup_inner(&mut self) -> crate::LocalResult<()> {
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
            let mut pending_path_bytes = self.path.as_os_str().as_encoded_bytes().len();
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
            }
            drop(self.child.take());
        }
        match self.parent.observe_entry(&self.relative) {
            Ok(observation)
                if self
                    .identity
                    .as_ref()
                    .is_some_and(|identity| observation.identity() == Some(identity)) =>
            {
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
    /// Removes the retained tree and disarms this owner when cleanup succeeds.
    ///
    /// On failure, returns an error that still owns this resource so cleanup
    /// can be retried or the owner can be transferred elsewhere.
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
    pub fn retain_in_place(self) -> LocalTreeRecoveryLocation {
        let mut this = self;
        this.cleanup_armed = false;
        LocalTreeRecoveryLocation::new(std::mem::take(&mut this.path), this.identity.take())
    }
}

fn cleanup_deadline_exceeded(limits: LocalTempCleanupLimits, started_at: std::time::Instant) -> bool {
    limits
        .deadline()
        .is_some_and(|deadline| started_at.elapsed() >= deadline)
}

impl Drop for LocalTreeRetainedResource {
    fn drop(&mut self) {
        if self.cleanup_armed {
            let _ = self.cleanup_inner();
        }
    }
}
