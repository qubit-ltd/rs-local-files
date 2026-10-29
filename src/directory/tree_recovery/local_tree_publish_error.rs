// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::super::staged::LocalStagedDirectory;
use super::local_tree_retained_resource::LocalTreeRetainedResource;
use crate::LocalFileError;
use crate::outcome::LocalTreeMutationState;

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
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::LocalTreePublishError;
/// use qubit_local_files::directory::{LocalStagedDirectory, LocalTreeCleanupError, LocalTreeRetainedResource};
/// use qubit_local_files::options::{LocalDirectoryOpenOptions, LocalTempDirectoryOptions, LocalTreePublishOptions};
/// use qubit_local_files::outcome::LocalTreePublication;
/// use std::error::Error;
/// use std::ffi::OsStr;
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
/// let fixture = tempfile::tempdir()?;
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
#[derive(Debug)]
pub struct LocalTreePublishError {
    cause: LocalFileError,
    stage: Option<LocalStagedDirectory>,
    retained: Vec<LocalTreeRetainedResource>,
    mutation: LocalTreeMutationState,
    publication: crate::outcome::LocalTreePublication,
    access_state: crate::outcome::LocalTreeAccessState,
}
impl LocalTreePublishError {
    pub(crate) fn new(
        cause: LocalFileError,
        stage: Option<LocalStagedDirectory>,
        mutation: LocalTreeMutationState,
        publication: crate::outcome::LocalTreePublication,
        access_state: crate::outcome::LocalTreeAccessState,
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
    pub(crate) fn with_retained(mut self, retained: Option<LocalTreeRetainedResource>) -> Self {
        if let Some(retained) = retained {
            self.retained.push(retained);
        }
        self
    }
    /// Returns the publication failure without consuming any recovery owner.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Returns the owner policy retained by the recovery stage, when known.
    pub fn owner_access_policy(&self) -> Option<&crate::policy::LocalOwnerAccessPolicy> {
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
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.mutation
    }
    /// Borrows retained cleanup owners; they remain governed by this error's
    /// Drop behavior until extracted.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Returns the publication outcome recorded before the failure.
    pub const fn publication(&self) -> crate::outcome::LocalTreePublication {
        self.publication
    }
    /// Returns the access state observed when publication failed.
    pub const fn access_state(&self) -> crate::outcome::LocalTreeAccessState {
        self.access_state
    }
    /// Transfers the optional stage. If it is `None`, no stage owner remains.
    /// Any unextracted retained owners are dropped and may attempt cleanup;
    /// use [`Self::into_parts`] to take all owners.
    pub fn into_stage(self) -> Option<LocalStagedDirectory> {
        self.stage
    }
    /// Transfers retained cleanup owners. The optional stage is dropped and
    /// may attempt best-effort cleanup; use [`Self::into_parts`] to transfer
    /// the stage as well.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the cause, optional stage, and retained cleanup owners.
    /// Inspect publication, mutation, and access state before calling this
    /// method because those status values are not part of the returned tuple.
    pub fn into_parts(
        mut self,
    ) -> (
        LocalFileError,
        Option<LocalStagedDirectory>,
        Vec<LocalTreeRetainedResource>,
    ) {
        (self.cause, self.stage.take(), std::mem::take(&mut self.retained))
    }
}
impl std::fmt::Display for LocalTreePublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tree publish failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreePublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
