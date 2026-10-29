// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::directory::LocalTreeRetainedResource;
use crate::error::LocalFileError;
use crate::outcome::LocalTreeAccessState;
use crate::outcome::LocalTreePublication;
use crate::policy::LocalOwnerAccessPolicy;

/// Reports independent publication, synchronization, and access facts for a
/// staged tree.
///
/// `publication` says whether the name was installed. `durable` reports the
/// requested parent namespace durability; `file_synced` reports requested
/// regular-file synchronization. A preferred sync can fail while publication
/// succeeds, so inspect these facts separately. `access_state` reports the
/// final access state confirmed by the publish operation. None of these fields
/// implies the values of the others.
///
/// # Errors
///
/// [`LocalStagedDirectory::publish_new`](crate::directory::LocalStagedDirectory::publish_new)
/// returns an error with publication and access state when validation,
/// synchronization required by policy, or native publication fails.
///
/// # Examples
///
/// ```
/// use std::ffi::OsStr;
/// use qubit_local_files::directory::LocalDirectoryHandle;
/// use qubit_local_files::options::{LocalDirectoryOpenOptions, LocalTempDirectoryOptions, LocalTreePublishOptions};
/// use qubit_local_files::policy::{LocalDurabilityRequirement, LocalFileSyncRequirement};
/// use qubit_local_files::outcome::LocalTreePublication;
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let temporary = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let parent = filesystem.open_directory_with_options(
///     temporary.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// let outcome = stage.publish_new(
///     &parent,
///     OsStr::new("published"),
///     &LocalTreePublishOptions::new()
///         .with_durability(LocalDurabilityRequirement::Preferred)
///         .with_file_sync(LocalFileSyncRequirement::Preferred),
/// )?;
/// assert_eq!(outcome.publication(), LocalTreePublication::Published);
/// let _namespace_durable = outcome.durable();
/// let _regular_files_synced = outcome.file_synced();
/// let _access_state = outcome.access_state();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
#[must_use = "inspect publication, synchronization, and access outcomes"]
pub struct LocalTreePublishOutcome {
    publication: LocalTreePublication,
    durable: bool,
    file_synced: bool,
    access_state: LocalTreeAccessState,
    cleanup_errors: Vec<LocalFileError>,
    retained: Vec<LocalTreeRetainedResource>,
    owner_access_policy: Option<LocalOwnerAccessPolicy>,
}
impl LocalTreePublishOutcome {
    pub(crate) fn set_owner_access_policy(&mut self, policy: Option<LocalOwnerAccessPolicy>) {
        self.owner_access_policy = policy;
    }
    pub(crate) fn new(
        publication: LocalTreePublication,
        durable: bool,
        file_synced: bool,
        access_state: LocalTreeAccessState,
    ) -> Self {
        Self {
            publication,
            durable,
            file_synced,
            access_state,
            cleanup_errors: Vec::new(),
            retained: Vec::new(),
            owner_access_policy: None,
        }
    }
    /// Returns whether publication completed.
    pub const fn publication(&self) -> LocalTreePublication {
        self.publication
    }
    /// Reports whether the requested parent namespace durability was achieved.
    pub const fn durable(&self) -> bool {
        self.durable
    }
    /// Reports whether requested regular file synchronization completed.
    pub const fn file_synced(&self) -> bool {
        self.file_synced
    }
    /// Returns the final access state confirmed when publication completed.
    pub const fn access_state(&self) -> LocalTreeAccessState {
        self.access_state
    }
    /// Returns the exact owner policy confirmed before publication, when
    /// applied.
    pub const fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
    /// Returns secondary cleanup errors observed after publication.
    pub fn cleanup_errors(&self) -> &[LocalFileError] {
        &self.cleanup_errors
    }
    /// Returns resources retained for explicit recovery.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers all retained recovery resources.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
}
