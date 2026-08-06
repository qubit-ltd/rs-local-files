// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recoverable temporary-resource persistence errors.
// qubit-style: allow source-test-pair

use std::error::Error;
use std::fmt::{Debug, Display, Formatter, Result as FmtResult};
use std::io;
use std::path::{Path, PathBuf};

use crate::{LocalFileError, LocalFileOperation, LocalPersistFailureState, LocalPersistStage};

/// Persistence error that returns ownership of the temporary resource.
///
/// The stage distinguishes target resolution, parent preparation, and final
/// installation. [`Self::requested_target`] always returns the caller's path;
/// [`Self::resolved_target`] returns the bound absolute path once resolution
/// has succeeded. The resource remains available for retry, inspection, keep,
/// or explicit cleanup only while the retained resource still has a known
/// owned namespace entry. After an indeterminate native publish failure,
/// temporary handles reject cleanup and their `Drop` implementation performs
/// no namespace operation.
#[non_exhaustive]
#[derive(Debug)]
pub struct LocalPersistError<T> {
    /// Structured local filesystem error that prevented persistence.
    error: Box<LocalFileError>,
    /// Temporary resource retained after the failed operation.
    resource: Box<T>,
    /// Target path supplied by the caller.
    requested_target: PathBuf,
    /// Absolute target path, when target resolution succeeded.
    resolved_target: Option<PathBuf>,
    /// Stage at which persistence failed.
    stage: LocalPersistStage,
    /// Strongest namespace state established by the failed operation.
    state: LocalPersistFailureState,
}

impl<T> LocalPersistError<T> {
    /// Creates a recoverable persistence error.
    ///
    /// # Parameters
    /// - `error`: Native I/O error that prevented persistence.
    /// - `resource`: Temporary resource retained after the failure.
    /// - `requested_target`: Target path supplied by the caller.
    /// - `resolved_target`: Absolute target, when resolution succeeded.
    /// - `stage`: Stage at which persistence failed.
    ///
    /// # Returns
    /// New persistence error owning both values.
    #[inline(always)]
    pub(crate) fn new(
        error: io::Error,
        resource: T,
        requested_target: PathBuf,
        resolved_target: Option<PathBuf>,
        stage: LocalPersistStage,
    ) -> Self {
        let state = LocalPersistFailureState::from_error(stage, error.kind());
        let error = LocalFileError::from_io(
            LocalFileOperation::PersistTemp,
            Some(requested_target.clone()),
            resolved_target.clone(),
            error,
        );
        Self {
            error: Box::new(error),
            resource: Box::new(resource),
            requested_target,
            resolved_target,
            stage,
            state,
        }
    }

    /// Returns the structured persistence error.
    ///
    /// # Returns
    /// Structured error that prevented persistence.
    #[must_use]
    pub const fn error(&self) -> &LocalFileError {
        &self.error
    }

    /// Returns the retained temporary resource.
    ///
    /// # Returns
    /// Shared reference to the resource retained after failure.
    #[must_use]
    pub const fn resource(&self) -> &T {
        &self.resource
    }

    /// Returns the retained temporary resource mutably.
    ///
    /// # Returns
    /// Mutable reference to the resource retained after failure.
    #[must_use]
    pub const fn resource_mut(&mut self) -> &mut T {
        &mut self.resource
    }

    /// Returns the target path supplied by the caller.
    ///
    /// # Returns
    /// Requested target before absolute-path resolution.
    #[must_use]
    pub fn requested_target(&self) -> &Path {
        &self.requested_target
    }

    /// Returns the resolved absolute target, when resolution succeeded.
    ///
    /// # Returns
    /// Resolved target for parent preparation and destination installation.
    pub fn resolved_target(&self) -> Option<&Path> {
        self.resolved_target.as_deref()
    }

    /// Returns the stage at which persistence failed.
    ///
    /// # Returns
    /// Failed persistence stage.
    pub const fn stage(&self) -> LocalPersistStage {
        self.stage
    }

    /// Returns the strongest namespace state established by the failure.
    ///
    /// # Returns
    /// A state describing whether the temporary resource remains safely owned.
    pub const fn state(&self) -> LocalPersistFailureState {
        self.state
    }

    /// Returns the stable persistence error kind.
    ///
    /// # Returns
    /// Stable classification reported by the retained structured error.
    pub const fn kind(&self) -> crate::LocalFileErrorKind {
        self.error.kind()
    }

    /// Splits this error into the original compatibility values.
    ///
    /// # Returns
    /// Native error, retained resource, requested target, resolved target, and
    /// failure stage. Use [`Self::into_parts_with_state`] to retain the
    /// publication state too.
    ///
    /// Ignoring the returned tuple is rejected because it owns the retained
    /// temporary resource:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// use qubit_local_files::LocalPersistError;
    ///
    /// fn discard(error: LocalPersistError<()>) {
    ///     error.into_parts();
    /// }
    /// ```
    #[must_use = "the returned tuple retains the temporary resource and persistence context"]
    pub fn into_parts(
        self,
    ) -> (
        LocalFileError,
        T,
        PathBuf,
        Option<PathBuf>,
        LocalPersistStage,
    ) {
        let (error, resource, requested_target, resolved_target, stage, _) =
            self.into_parts_with_state();
        (error, resource, requested_target, resolved_target, stage)
    }

    /// Splits this error into its retained values, including recovery state.
    ///
    /// # Returns
    ///
    /// The native error, temporary resource, requested target, resolved target,
    /// failure stage, and publication state in that order.
    #[must_use = "the returned tuple retains the temporary resource and persistence context"]
    pub fn into_parts_with_state(
        self,
    ) -> (
        LocalFileError,
        T,
        PathBuf,
        Option<PathBuf>,
        LocalPersistStage,
        LocalPersistFailureState,
    ) {
        let Self {
            error,
            resource,
            requested_target,
            resolved_target,
            stage,
            state,
        } = self;
        (
            *error,
            *resource,
            requested_target,
            resolved_target,
            stage,
            state,
        )
    }
}

impl<T> Display for LocalPersistError<T> {
    /// Formats the failure stage, target context, and native error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        if let Some(resolved_target) = self.resolved_target.as_ref() {
            write!(
                formatter,
                "failed to persist temporary resource during {:?} to requested \
                 target '{}' (resolved as '{}'): {}",
                self.stage,
                self.requested_target.display(),
                resolved_target.display(),
                self.error,
            )
        } else {
            write!(
                formatter,
                "failed to persist temporary resource during {:?} to requested \
                 target '{}': {}",
                self.stage,
                self.requested_target.display(),
                self.error,
            )
        }
    }
}

impl<T> Error for LocalPersistError<T>
where
    T: Debug,
{
    /// Returns the retained structured error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}
