// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::error::Error;
use std::fmt;

use super::super::staged::LocalStagedDirectory;
use super::local_tree_retained_resource::LocalTreeRetainedResource;
use crate::LocalFileError;
use crate::outcome::LocalTreeAccessState;
use crate::outcome::LocalTreeMutationState;
use crate::outcome::LocalTreePublication;
use crate::policy::LocalOwnerAccessPolicy;

/// Failure while publishing a staged directory.
///
/// The error owns an unpublished stage and any retained cleanup resources.
/// Inspect publication, mutation, and access state before consuming it, then
/// use [`Self::into_parts`] to take every transferable owner. Dropping an
/// unextracted stage or armed retained owner can trigger best-effort cleanup.
/// The borrowed cause is also exposed as the standard error source.
///
/// # Examples
///
/// The conflict path below inspects the state first, extracts all owners, and
/// explicitly cleans each one. Cleanup errors are returned with their owner
/// and any unprocessed owners still available for retry.
///
/// ```
/// use std::error::Error;
/// use std::ffi::OsStr;
///
/// use tempfile::tempdir;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::LocalTreePublishError;
/// use qubit_local_files::directory::LocalStagedDirectory;
/// use qubit_local_files::directory::LocalTreeCleanupError;
/// use qubit_local_files::directory::LocalTreeRetainedResource;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
/// use qubit_local_files::options::LocalTempDirectoryOptions;
/// use qubit_local_files::options::LocalTreePublishOptions;
/// use qubit_local_files::outcome::LocalTreePublication;
///
/// #[derive(Debug)]
/// struct RecoveryCleanupFailure {
///     failure: LocalTreeCleanupError,
///     remaining: Vec<LocalTreeRetainedResource>,
/// }
///
/// impl std::fmt::Display for RecoveryCleanupFailure {
///     fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         write!(formatter, "cleanup failed with {} retained resources remaining: {}", self.remaining.len(), self.failure)
///     }
/// }
///
/// impl Error for RecoveryCleanupFailure {
///     fn source(&self) -> Option<&(dyn Error + 'static)> {
///         Some(&self.failure)
///     }
/// }
///
/// fn cleanup_all(
///     stage: Option<LocalStagedDirectory>,
///     retained: Vec<LocalTreeRetainedResource>,
/// ) -> Result<(), RecoveryCleanupFailure> {
///     if let Some(stage) = stage {
///         if let Err(failure) = stage.cleanup() {
///             return Err(RecoveryCleanupFailure { failure, remaining: retained });
///         }
///     }
///     let mut remaining = retained.into_iter();
///     while let Some(resource) = remaining.next() {
///         if let Err(failure) = resource.cleanup() {
///             return Err(RecoveryCleanupFailure {
///                 failure,
///                 remaining: remaining.collect(),
///             });
///         }
///     }
///     Ok(())
/// }
///
/// fn recover(failure: LocalTreePublishError) -> Result<(), RecoveryCleanupFailure> {
///     let _publication = failure.publication();
///     let _mutation = failure.mutation_state();
///     let _access = failure.access_state();
///     let (_, stage, retained) = failure.into_parts();
///     cleanup_all(stage, retained)
/// }
///
/// let fixture = tempdir()?;
/// std::fs::create_dir(fixture.path().join("target"))?;
/// let filesystem = LocalFileSystem::host()?;
/// let parent = filesystem.open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())?;
/// let stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// let failure = stage.publish_new(&parent, OsStr::new("target"), &LocalTreePublishOptions::new())
///     .expect_err("existing target should conflict");
/// assert_eq!(LocalTreePublication::NotPublished, failure.publication());
/// recover(failure)?;
/// # Ok::<(), Box<dyn Error>>(())
/// ```
#[must_use = "inspect or recover the failed publication and its cleanup owners"]
#[derive(Debug)]
pub struct LocalTreePublishError {
    /// Structured cause that explains why publication failed.
    cause: LocalFileError,
    /// Staged directory owner, if publication did not consume it.
    stage: Option<LocalStagedDirectory>,
    /// Cleanup owners transferred into this error during recovery.
    retained: Vec<LocalTreeRetainedResource>,
    /// Whether the failed operation may have changed the destination.
    mutation: LocalTreeMutationState,
    /// Publication result observed before constructing this error.
    publication: LocalTreePublication,
    /// Access-policy state observed when publication failed.
    access_state: LocalTreeAccessState,
}
impl LocalTreePublishError {
    /// Creates a publication error while retaining any stage owner for
    /// recovery.
    ///
    /// The error starts without additional retained cleanup resources.
    ///
    /// # Parameters
    ///
    /// * `cause` is the structured filesystem failure.
    /// * `stage` is the staged directory owner, if it remains available; `None`
    ///   records that no stage owner remains.
    /// * `mutation` records possible destination or tree changes.
    /// * `publication` records whether the stage was published.
    /// * `access_state` records the known access-policy state.
    ///
    /// # Returns
    ///
    /// An error that owns `stage` and can later transfer its recovery owners.
    pub(crate) fn new(
        cause: LocalFileError,
        stage: Option<LocalStagedDirectory>,
        mutation: LocalTreeMutationState,
        publication: LocalTreePublication,
        access_state: LocalTreeAccessState,
    ) -> Self {
        Self {
            cause,
            stage,
            retained: Vec::new(),
            mutation,
            publication,
            access_state,
        }
    }
    /// Returns the publication failure without consuming any recovery owner.
    ///
    /// # Returns
    ///
    /// A borrow of the structured filesystem cause.
    #[must_use = "inspect the structured cause of the publication failure"]
    #[inline]
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Returns the owner policy retained by the recovery stage, when known.
    ///
    /// # Returns
    ///
    /// `Some` contains the known policy; `None` means no retained owner exposes
    /// one.
    #[must_use = "inspect the policy applied to the recovery owner"]
    pub fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.stage
            .as_ref()
            .and_then(|stage| stage.owner_access_policy())
            .or_else(|| {
                self.retained
                    .iter()
                    .find_map(LocalTreeRetainedResource::owner_access_policy)
            })
    }
    /// Returns whether publication may have mutated the destination namespace.
    ///
    /// # Returns
    ///
    /// The recorded mutation state, including `Indeterminate` when effects
    /// could not be established.
    #[must_use = "inspect whether publication may have changed the destination"]
    #[inline]
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.mutation
    }
    /// Borrows retained cleanup owners; they remain governed by this error's
    /// Drop behavior until extracted.
    ///
    /// # Returns
    ///
    /// A borrowed slice of retained owners; the slice is empty when none were
    /// retained.
    #[must_use = "inspect retained cleanup owners before consuming the error"]
    #[inline]
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Returns the publication outcome recorded before the failure.
    ///
    /// # Returns
    ///
    /// The recorded outcome, including `Indeterminate` when publication could
    /// not be established.
    #[must_use = "inspect whether publication completed before recovering"]
    #[inline]
    pub const fn publication(&self) -> LocalTreePublication {
        self.publication
    }
    /// Returns the access state observed when publication failed.
    ///
    /// # Returns
    ///
    /// The observed access state, including `Indeterminate` when it is unknown.
    #[must_use = "inspect the access state before recovering the stage"]
    #[inline]
    pub const fn access_state(&self) -> LocalTreeAccessState {
        self.access_state
    }
    /// Transfers the optional stage. If it is `None`, no stage owner remains.
    /// Any unextracted retained owners are dropped and may attempt cleanup;
    /// use [`Self::into_parts`] to take all owners.
    ///
    /// # Returns
    ///
    /// `Some` transfers the staged-directory owner; `None` means no stage was
    /// retained. Other retained owners are dropped with the error.
    #[inline]
    pub fn into_stage(self) -> Option<LocalStagedDirectory> {
        self.stage
    }
    /// Transfers retained cleanup owners. The optional stage is dropped and
    /// may attempt best-effort cleanup; use [`Self::into_parts`] to transfer
    /// the stage as well.
    ///
    /// # Returns
    ///
    /// All retained cleanup owners. The stage owner is not included and is
    /// dropped with the consumed error.
    #[must_use = "retain or explicitly clean the transferred recovery owners"]
    #[inline]
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the cause, optional stage, and retained cleanup owners.
    /// Inspect publication, mutation, and access state before calling this
    /// method because those status values are not part of the returned tuple.
    ///
    /// # Returns
    ///
    /// The cause, optional stage owner, and all retained cleanup owners, in
    /// that order. `None` for the stage means no stage owner was retained.
    #[must_use = "handle the cause and preserve or clean each transferred owner"]
    #[inline]
    pub fn into_parts(
        mut self,
    ) -> (
        LocalFileError,
        Option<LocalStagedDirectory>,
        Vec<LocalTreeRetainedResource>,
    ) {
        (self.cause, self.stage.take(), std::mem::take(&mut self.retained))
    }
    /// Adds a cleanup owner produced while handling the publication failure.
    ///
    /// `None` leaves the retained-owner list unchanged.
    ///
    /// # Parameters
    ///
    /// * `retained` is an additional cleanup owner, when recovery produced one.
    ///
    /// # Returns
    ///
    /// This error with the supplied owner appended when present.
    pub(crate) fn with_retained(mut self, retained: Option<LocalTreeRetainedResource>) -> Self {
        if let Some(retained) = retained {
            self.retained.push(retained);
        }
        self
    }
}
impl fmt::Display for LocalTreePublishError {
    /// Formats a concise description while leaving recovery owners untouched.
    ///
    /// # Parameters
    ///
    /// * `f` is the destination formatter that receives the error description.
    ///
    /// # Errors
    ///
    /// Returns an error if writing the description to the formatter fails.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tree publish failed: {}", self.cause)
    }
}
impl Error for LocalTreePublishError {
    /// Exposes the structured file error as the underlying cause.
    ///
    /// # Returns
    ///
    /// `Some` contains the structured filesystem cause; this implementation
    /// never returns `None`.
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod recovery_ownership_tests {
    use std::error::Error;
    use std::path::Path;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use crate::LocalFileError;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::directory::LocalDirectoryHandle;
    use crate::directory::LocalStagedDirectory;
    use crate::directory::LocalTreeCreateError;
    use crate::directory::LocalTreePublishError;
    use crate::directory::LocalTreeRetainedResource;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::outcome::LocalTreeAccessState;
    use crate::outcome::LocalTreeMutationState;
    use crate::outcome::LocalTreePublication;
    use crate::policy::LocalOwnerAccessPolicy;

    fn parent(path: &Path) -> LocalDirectoryHandle {
        LocalFileSystem::host()
            .expect("host filesystem")
            .open_directory_with_options(path, &LocalDirectoryOpenOptions::new())
            .expect("fixture parent authority")
    }

    fn stage(parent: &LocalDirectoryHandle) -> LocalStagedDirectory {
        let mut stage = parent
            .create_staged_directory(
                &LocalTempDirectoryOptions::new()
                    .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600)),
            )
            .expect("stage with owner policy");
        stage
            .write_file_new(Path::new("payload"), &mut &b"tracked"[..], 7)
            .expect("tracked contents");
        stage
    }

    fn retained(parent: &LocalDirectoryHandle) -> (LocalTreeRetainedResource, PathBuf) {
        let mut stage = stage(parent);
        let path = stage.directory().diagnostic_path().to_path_buf();
        let owner = stage.owner.take().expect("stage cleanup owner");
        drop(stage);
        assert!(path.exists(), "moving its owner must preserve the staged tree");
        (owner, path)
    }

    fn cause(path: &Path) -> LocalFileError {
        let source = std::fs::File::open(path.join("missing")).expect_err("fixture has no missing entry");
        LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
    }

    #[test]
    fn test_publish_extracting_retained_owners_drops_unextracted_stage_only() {
        let scratch = tempdir().expect("scratch");
        let parent = parent(scratch.path());
        let stage = stage(&parent);
        let stage_path = stage.directory().diagnostic_path().to_path_buf();
        let (owner, retained_path) = retained(&parent);
        let error = LocalTreePublishError::new(
            cause(scratch.path()),
            Some(stage),
            LocalTreeMutationState::NotStarted,
            LocalTreePublication::NotPublished,
            LocalTreeAccessState::Preserved,
        )
        .with_retained(Some(owner))
        .with_retained(None);
        assert_eq!(1, error.retained_resources().len());
        let source = error.source().expect("primary structured source");
        assert!(std::ptr::eq(
            source.downcast_ref::<LocalFileError>().expect("structured cause"),
            error.cause()
        ));
        assert_eq!(format!("tree publish failed: {}", error.cause()), error.to_string());
        assert_eq!(
            Some(&LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600)),
            error.owner_access_policy()
        );
        let owners = error.into_retained_resources();
        let debug = format!("{:?}", owners[0]);
        assert!(debug.contains("LocalTreeRetainedResource"));
        assert!(
            !debug.contains(scratch.path().to_str().expect("UTF-8 fixture path")),
            "retained-resource debug output must redact filesystem paths"
        );
        assert!(
            !stage_path.exists(),
            "unextracted stage is cleaned when error is consumed"
        );
        assert!(
            retained_path.join("payload").exists(),
            "extracted owner keeps its tree alive"
        );
        drop(owners);
        assert!(!retained_path.exists(), "transferred owner remains cleanup-armed");
    }

    #[test]
    fn test_publish_retained_policy_survives_absent_stage_and_lossless_extraction() {
        let scratch = tempdir().expect("scratch");
        let parent = parent(scratch.path());
        let (owner, path) = retained(&parent);
        let policy = *owner.owner_access_policy().expect("retained policy");
        let error = LocalTreePublishError::new(
            cause(scratch.path()),
            None,
            LocalTreeMutationState::Indeterminate,
            LocalTreePublication::Indeterminate,
            LocalTreeAccessState::OwnerPolicyApplied,
        )
        .with_retained(Some(owner));
        assert_eq!(Some(&policy), error.owner_access_policy());
        let (cause, stage, owners) = error.into_parts();
        assert_eq!(LocalFileErrorKind::NotFound, cause.kind());
        assert!(stage.is_none());
        assert_eq!(1, owners.len());
        assert!(path.exists());
        owners
            .into_iter()
            .next()
            .expect("retained owner")
            .cleanup()
            .expect("explicit cleanup");
        assert!(!path.exists());
    }

    #[test]
    fn test_create_extracting_stage_drops_only_unextracted_retained_owner() {
        let scratch = tempdir().expect("scratch");
        let parent = parent(scratch.path());
        let stage = stage(&parent);
        let stage_path = stage.directory().diagnostic_path().to_path_buf();
        let identity = stage.identity().cloned();
        let (owner, retained_path) = retained(&parent);
        let error = LocalTreeCreateError::new(
            cause(scratch.path()),
            Some(stage),
            vec![owner],
            LocalTreeMutationState::Started,
        );
        assert_eq!(stage_path.file_name(), error.stage_name());
        assert_eq!(identity.as_ref(), error.stage_identity());
        assert_eq!(
            format!("staged directory creation failed: {}", error.cause()),
            error.to_string()
        );
        assert!(std::ptr::eq(
            error
                .source()
                .expect("source")
                .downcast_ref::<LocalFileError>()
                .expect("structured cause"),
            error.cause()
        ));
        let stage = error.into_stage().expect("stage transferred");
        assert!(stage_path.join("payload").exists());
        assert!(!retained_path.exists(), "unextracted retained owner is cleaned");
        stage.cleanup().expect("extracted stage can be explicitly cleaned");
        assert!(!stage_path.exists());
    }

    #[test]
    fn test_create_retained_extraction_discards_stage_and_preserves_partial_owner_identity() {
        let scratch = tempdir().expect("scratch");
        let parent = parent(scratch.path());
        let stage = stage(&parent);
        let stage_path = stage.directory().diagnostic_path().to_path_buf();
        let (owner, retained_path) = retained(&parent);
        let error = LocalTreeCreateError::new(
            cause(scratch.path()),
            Some(stage),
            vec![owner],
            LocalTreeMutationState::Started,
        );
        let mut owners = error.into_retained_resources();
        assert!(!stage_path.exists());
        assert!(retained_path.exists());
        let owner = owners.pop().expect("partial resource");
        let identity = owner.identity().cloned();
        let error = LocalTreeCreateError::new(
            cause(scratch.path()),
            None,
            vec![owner],
            LocalTreeMutationState::Started,
        );
        assert_eq!(retained_path.file_name(), error.stage_name());
        assert_eq!(identity.as_ref(), error.stage_identity());
        let (_, stage, owners) = error.into_parts();
        assert!(stage.is_none());
        drop(owners);
        assert!(!retained_path.exists());
        let error = LocalTreeCreateError::new(
            cause(scratch.path()),
            None,
            Vec::new(),
            LocalTreeMutationState::NotStarted,
        );
        assert!(
            error.stage_identity().is_none(),
            "missing authority must not fabricate an identity"
        );
        assert_eq!(scratch.path().file_name(), error.stage_name());
    }
}
