// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use super::super::staged::LocalStagedDirectory;
use super::local_tree_retained_resource::LocalTreeRetainedResource;
use crate::LocalFileError;
use crate::observation::LocalFileIdentity;
use crate::outcome::LocalTreeMutationState;

/// Failure while creating a staged directory.
///
/// The error owns any stage and retained cleanup resources until they are
/// transferred or the error is dropped. Dropping an unextracted stage or an
/// armed retained owner can trigger its best-effort cleanup.
/// Its cause is borrowed by [`Self::cause`] and exposed as the standard error
/// source.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeCreateError;
/// use qubit_local_files::directory::LocalStagedDirectory;
///
/// fn parts(error: LocalTreeCreateError) -> (
///     qubit_local_files::LocalFileError,
///     Option<LocalStagedDirectory>,
///     Vec<qubit_local_files::directory::LocalTreeRetainedResource>,
/// ) {
///     error.into_parts()
/// }
/// ```
#[derive(Debug)]
pub struct LocalTreeCreateError {
    /// Primary cause describing why stage creation stopped.
    cause: LocalFileError,
    /// Partially created stage owner, when the child authority was established.
    stage: Option<LocalStagedDirectory>,
    /// Additional cleanup owners for native resources retained after failure.
    retained: Vec<LocalTreeRetainedResource>,
    /// Whether creation had already mutated the namespace before failing.
    state: LocalTreeMutationState,
}
impl LocalTreeCreateError {
    pub(crate) fn new(
        cause: LocalFileError,
        stage: Option<LocalStagedDirectory>,
        retained: Vec<LocalTreeRetainedResource>,
        state: LocalTreeMutationState,
    ) -> Self {
        Self {
            cause,
            stage,
            retained,
            state,
        }
    }
    /// Returns the primary stage creation failure, preserving its operation and
    /// any native error context.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Reports whether stage creation changed the namespace before failing.
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.state
    }
    /// Returns owners for any partially created stage resources.
    ///
    /// This only borrows them; they remain owned by this error.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers all retained resources to the caller.
    ///
    /// The optional stage is left to be dropped and may run its best-effort
    /// cleanup. Explicitly extract it with [`Self::into_parts`] when needed.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the partially created stage, if its child authority was
    /// established.
    ///
    /// Any retained resources left in this error are dropped and may attempt
    /// cleanup. `None` means no stage owner was established. Use
    /// [`Self::into_parts`] to transfer all owned parts together.
    pub fn into_stage(self) -> Option<LocalStagedDirectory> {
        self.stage
    }
    /// Transfers the cause, optional stage, and all retained cleanup owners.
    ///
    /// This is the lossless extraction path for every transferable part.
    pub fn into_parts(
        self,
    ) -> (
        LocalFileError,
        Option<LocalStagedDirectory>,
        Vec<LocalTreeRetainedResource>,
    ) {
        (self.cause, self.stage, self.retained)
    }
    /// Returns the name of a fully or partially created stage, when known.
    ///
    /// `None` means neither the stage owner, retained resources, nor primary
    /// cause supplied a diagnostic basename.
    pub fn stage_name(&self) -> Option<&std::ffi::OsStr> {
        self.stage
            .as_ref()
            .map(LocalStagedDirectory::relative_name)
            .or_else(|| {
                self.retained
                    .first()
                    .and_then(|resource| resource.diagnostic_path().file_name())
            })
            .or_else(|| self.cause.path().and_then(Path::file_name))
    }
    /// Returns the identity of a fully or partially created stage, when known.
    ///
    /// `None` means no retained stage or recovery owner had a native identity.
    pub fn stage_identity(&self) -> Option<&LocalFileIdentity> {
        self.stage
            .as_ref()
            .and_then(LocalStagedDirectory::identity)
            .or_else(|| self.retained.first().and_then(LocalTreeRetainedResource::identity))
            .or_else(|| self.cause.temp_recovery().and_then(crate::LocalTempRecovery::identity))
    }
}
impl std::fmt::Display for LocalTreeCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "staged directory creation failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreeCreateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
