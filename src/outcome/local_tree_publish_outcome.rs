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
/// let publication = stage.publish_new(
///     &parent,
///     OsStr::new("published"),
///     &LocalTreePublishOptions::new()
///         .with_durability(LocalDurabilityRequirement::Preferred)
///         .with_file_sync(LocalFileSyncRequirement::Preferred),
/// );
/// #[cfg(target_os = "freebsd")]
/// {
///     let failure = publication.expect_err("FreeBSD tree publication is unsupported");
///     assert_eq!(failure.cause().kind(), qubit_local_files::error::LocalFileErrorKind::RequirementNotMet);
///     assert_eq!(failure.publication(), LocalTreePublication::NotPublished);
///     let (_, stage, retained) = failure.into_parts();
///     if let Some(stage) = stage {
///         stage.cleanup()?;
///     }
///     for resource in retained {
///         resource.cleanup()?;
///     }
/// }
/// #[cfg(not(target_os = "freebsd"))]
/// {
///     let outcome = publication?;
///     assert_eq!(outcome.publication(), LocalTreePublication::Published);
///     let _namespace_durable = outcome.durable();
///     let _regular_files_synced = outcome.file_synced();
///     let _access_state = outcome.access_state();
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
#[must_use = "inspect publication, synchronization, and access outcomes"]
pub struct LocalTreePublishOutcome {
    /// Whether the destination name was installed by the publish operation.
    publication: LocalTreePublication,
    /// Whether requested parent namespace durability was achieved.
    durable: bool,
    /// Whether requested regular-file synchronization completed.
    file_synced: bool,
    /// Access state confirmed for the published tree.
    access_state: LocalTreeAccessState,
    /// Secondary cleanup failures that did not retract publication.
    cleanup_errors: Vec<LocalFileError>,
    /// Recovery owners retained after secondary cleanup could not complete.
    retained: Vec<LocalTreeRetainedResource>,
    /// Exact owner policy confirmed before publication, when one was applied.
    owner_access_policy: Option<LocalOwnerAccessPolicy>,
}
impl LocalTreePublishOutcome {
    /// Creates an outcome with no secondary cleanup failures or retained
    /// recovery owners.
    ///
    /// # Parameters
    ///
    /// - `publication`: Whether the destination name was installed.
    /// - `durable`: Whether requested parent namespace durability was achieved.
    /// - `file_synced`: Whether requested regular-file synchronization
    ///   completed.
    /// - `access_state`: Access state confirmed for the tree.
    ///
    /// # Returns
    ///
    /// Returns an outcome initialized with the supplied publication facts.
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
    /// Returns whether publication completed under the destination name.
    #[inline]
    pub const fn publication(&self) -> LocalTreePublication {
        self.publication
    }
    /// Reports whether the requested parent namespace durability was achieved.
    #[inline]
    pub const fn durable(&self) -> bool {
        self.durable
    }
    /// Reports whether requested regular file synchronization completed.
    #[inline]
    pub const fn file_synced(&self) -> bool {
        self.file_synced
    }
    /// Returns the final access state confirmed when publication completed.
    #[inline]
    pub const fn access_state(&self) -> LocalTreeAccessState {
        self.access_state
    }
    /// Returns the exact owner policy confirmed before publication, when
    /// applied.
    ///
    /// # Returns
    ///
    /// Returns `Some` when an exact owner policy was applied and confirmed, or
    /// `None` when publication did not use one.
    #[must_use = "inspect the owner access policy confirmed for the published tree"]
    #[inline]
    pub const fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
    /// Returns secondary cleanup errors observed after publication.
    #[inline]
    pub fn cleanup_errors(&self) -> &[LocalFileError] {
        &self.cleanup_errors
    }
    /// Returns resources retained for explicit recovery.
    #[inline]
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers ownership of all resources retained for explicit recovery.
    ///
    /// The returned vector is empty when no secondary cleanup resource was
    /// retained. Taking the resources leaves this outcome empty before it is
    /// dropped, so their ownership passes to the caller.
    #[must_use = "the retained resources must be kept for explicit recovery"]
    #[inline]
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }

    /// Records the owner policy confirmed by the publication operation.
    ///
    /// This crate-internal update preserves the policy for later outcome
    /// inspection without changing publication, synchronization, or cleanup
    /// state.
    ///
    /// # Parameters
    ///
    /// - `policy`: `Some` stores the confirmed exact policy; `None` records
    ///   that no owner policy was applied.
    #[inline]
    pub(crate) fn set_owner_access_policy(&mut self, policy: Option<LocalOwnerAccessPolicy>) {
        self.owner_access_policy = policy;
    }
}

#[cfg(test)]
mod recovery_ownership_tests {
    use std::ffi::OsStr;
    use std::fs::read;
    use std::path::Path;

    use tempfile::tempdir;

    use crate::LocalFileErrorKind;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempCleanupLimits;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTreePublishOptions;
    use crate::outcome::LocalTreePublication;

    #[test]
    fn test_published_outcome_transfers_secondary_cleanup_recovery_without_retracting_publication() {
        let scratch = tempdir().expect("scratch");
        let parent = LocalFileSystem::host()
            .expect("host filesystem")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent authority");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("publish stage");
        stage
            .write_file_new(Path::new("payload"), &mut &b"published"[..], 9)
            .expect("published contents");
        let mut outcome = stage
            .publish_new(&parent, OsStr::new("published"), &LocalTreePublishOptions::new())
            .expect("real successful publication");
        let mut leftover = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("secondary recovery fixture");
        leftover
            .write_file_new(Path::new("temporary"), &mut &b"leftover"[..], 8)
            .expect("tracked leftover");
        let leftover_path = scratch.path().join(leftover.relative_name());
        leftover.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0));
        let error = leftover.cleanup().expect_err("secondary cleanup exceeds its budget");
        let (cause, owners) = error.into_parts();
        outcome.cleanup_errors.push(cause);
        outcome.retained.extend(owners);

        assert_eq!(LocalTreePublication::Published, outcome.publication());
        assert!(
            !outcome.durable(),
            "default publication does not request parent durability"
        );
        assert_eq!(1, outcome.cleanup_errors().len());
        assert_eq!(LocalFileErrorKind::ResourceLimit, outcome.cleanup_errors()[0].kind());
        assert_eq!(1, outcome.retained_resources().len());
        let mut owners = outcome.into_retained_resources();
        assert!(
            leftover_path.exists(),
            "extraction preserves the secondary recovery tree"
        );
        let mut owner = owners.pop().expect("secondary owner");
        owner.set_cleanup_limits(LocalTempCleanupLimits::new());
        owner.cleanup().expect("retry secondary cleanup");
        assert!(!leftover_path.exists());
        assert_eq!(
            b"published",
            read(scratch.path().join("published/payload"))
                .expect("published file remains")
                .as_slice()
        );
    }
}
