// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::directory::LocalTreeValidationOptions;
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalFileSyncRequirement;
use crate::policy::LocalOwnerAccessPolicy;
use crate::policy::LocalTreeFinalAccess;

/// Optional verification, access, file synchronization, and durability
/// requested before tree publication.
///
/// `with_access` and `with_owner_access_policy` store independent values; a
/// later builder call does not clear an earlier one. When an owner policy is
/// set, `access` must remain [`LocalTreeFinalAccess::Preserve`]. A conflicting
/// combination is rejected by
/// [`crate::directory::LocalStagedDirectory::publish_new`] with
/// `InvalidOptions` before mutation. The error returns the unpublished stage
/// for explicit cleanup or retention; the builder itself is infallible.
///
/// | Owner policy | Access | Publication behavior |
/// | --- | --- | --- |
/// | `None` | Any supported value | Existing access-policy behavior |
/// | `Some(policy)` | `Preserve` | Apply the exact owner policy, subject to platform and value validation |
/// | `Some(policy)` | Any non-`Preserve` value | `InvalidOptions`; stage retained, mutation not started, publication not performed |
///
/// # Errors
///
/// See [`crate::directory::LocalStagedDirectory::publish_new`] for errors and
/// recovery state. This options type does not return errors while it is being
/// built.
///
/// # Examples
///
/// ```
/// use std::ffi::OsStr;
/// use qubit_local_files::{LocalFileSystem, LocalTreePublishError};
/// use qubit_local_files::error::LocalFileErrorKind;
/// use qubit_local_files::options::{LocalTempDirectoryOptions, LocalTreePublishOptions};
/// use qubit_local_files::outcome::{LocalTreeMutationState, LocalTreePublication};
/// use qubit_local_files::policy::{LocalOwnerAccessPolicy, LocalTreeFinalAccess};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let parent = filesystem.open_directory_with_options(
///     fixture.path(),
///     &qubit_local_files::options::LocalDirectoryOpenOptions::new(),
/// )?;
///
/// // Ordinary publication preserves the stage's existing access settings.
/// let stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// stage.publish_new(&parent, OsStr::new("published"), &LocalTreePublishOptions::new())?;
/// assert!(fixture.path().join("published").is_dir());
///
/// // These independent builder values conflict; publish_new reports it and
/// // returns the stage without starting mutation or publication.
/// let stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// let options = LocalTreePublishOptions::new()
///     .with_owner_access_policy(LocalOwnerAccessPolicy::new())
///     .with_access(LocalTreeFinalAccess::CurrentUserOnly);
/// let failure: LocalTreePublishError = stage
///     .publish_new(&parent, OsStr::new("conflict"), &options)
///     .expect_err("owner policy and non-Preserve access are incompatible");
/// assert_eq!(failure.cause().kind(), LocalFileErrorKind::InvalidOptions);
/// assert_eq!(failure.mutation_state(), LocalTreeMutationState::NotStarted);
/// assert_eq!(failure.publication(), LocalTreePublication::NotPublished);
/// let (_, stage, retained) = failure.into_parts();
/// assert!(retained.is_empty());
/// stage.expect("unpublished stage is returned").cleanup()?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct LocalTreePublishOptions {
    validation: Option<LocalTreeValidationOptions>,
    durability: LocalDurabilityRequirement,
    file_sync: LocalFileSyncRequirement,
    access: LocalTreeFinalAccess,
    owner_access_policy: Option<LocalOwnerAccessPolicy>,
}
impl LocalTreePublishOptions {
    /// Creates no-replace options that preserve access and request no
    /// synchronization.
    pub const fn new() -> Self {
        Self {
            validation: None,
            durability: LocalDurabilityRequirement::NotRequired,
            file_sync: LocalFileSyncRequirement::NotRequired,
            access: LocalTreeFinalAccess::Preserve,
            owner_access_policy: None,
        }
    }
    /// Validates the staged tree before any access or publication mutation.
    pub fn with_validation(mut self, validation: LocalTreeValidationOptions) -> Self {
        self.validation = Some(validation);
        self
    }
    /// Sets the requested parent namespace durability.
    pub const fn with_durability(mut self, requirement: LocalDurabilityRequirement) -> Self {
        self.durability = requirement;
        self
    }
    /// Sets the requested synchronization policy for regular files in the
    /// stage.
    pub const fn with_file_sync(mut self, requirement: LocalFileSyncRequirement) -> Self {
        self.file_sync = requirement;
        self
    }
    /// Sets the access policy applied to the stage before publication.
    ///
    /// This only stores the requested value; it does not clear an owner policy
    /// previously set by [`Self::with_owner_access_policy`]. If both are set
    /// and this value is not `Preserve`,
    /// [`crate::directory::LocalStagedDirectory::publish_new`]
    /// returns `InvalidOptions` before mutation and returns the stage in its
    /// error.
    pub const fn with_access(mut self, access: LocalTreeFinalAccess) -> Self {
        self.access = access;
        self
    }
    /// Sets an exact owner policy to apply before publication.
    ///
    /// This only stores the policy; it does not clear an access value
    /// previously set by [`Self::with_access`]. The combination is valid only
    /// with `Preserve`; [`crate::directory::LocalStagedDirectory::publish_new`]
    /// checks this before mutation and returns an `InvalidOptions` error
    /// with the stage retained when the values conflict.
    pub const fn with_owner_access_policy(mut self, policy: LocalOwnerAccessPolicy) -> Self {
        self.owner_access_policy = Some(policy);
        self
    }
    pub(crate) const fn validation(&self) -> Option<&LocalTreeValidationOptions> {
        self.validation.as_ref()
    }
    pub(crate) const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }
    pub(crate) const fn file_sync(&self) -> LocalFileSyncRequirement {
        self.file_sync
    }
    pub(crate) const fn access(&self) -> LocalTreeFinalAccess {
        self.access
    }
    pub(crate) const fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
}
impl Default for LocalTreePublishOptions {
    fn default() -> Self {
        Self::new()
    }
}
