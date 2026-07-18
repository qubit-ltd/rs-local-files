// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recoverable temporary-resource persistence errors.

use std::error::Error;
use std::fmt::{
    Debug,
    Display,
    Formatter,
    Result as FmtResult,
};
use std::io;
use std::path::{
    Path,
    PathBuf,
};

use crate::LocalPersistStage;

/// Persistence error that returns ownership of the temporary resource.
///
/// The stage distinguishes target resolution, parent preparation, and final
/// installation. [`Self::requested_target`] always returns the caller's path;
/// [`Self::resolved_target`] returns the bound absolute path once resolution
/// has succeeded. The resource remains available for retry, inspection, keep,
/// or explicit cleanup at every stage.
#[non_exhaustive]
#[derive(Debug)]
pub struct LocalPersistError<T> {
    /// Native I/O error that prevented persistence.
    error: io::Error,
    /// Temporary resource retained after the failed operation.
    resource: Box<T>,
    /// Target path supplied by the caller.
    requested_target: PathBuf,
    /// Absolute target path, when target resolution succeeded.
    resolved_target: Option<PathBuf>,
    /// Stage at which persistence failed.
    stage: LocalPersistStage,
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
    #[inline]
    pub(crate) fn new(
        error: io::Error,
        resource: T,
        requested_target: PathBuf,
        resolved_target: Option<PathBuf>,
        stage: LocalPersistStage,
    ) -> Self {
        Self {
            error,
            resource: Box::new(resource),
            requested_target,
            resolved_target,
            stage,
        }
    }

    /// Returns the native persistence error.
    ///
    /// # Returns
    /// I/O error that prevented persistence.
    #[inline(always)]
    pub const fn error(&self) -> &io::Error {
        &self.error
    }

    /// Returns the retained temporary resource.
    ///
    /// # Returns
    /// Shared reference to the resource retained after failure.
    #[inline(always)]
    pub const fn resource(&self) -> &T {
        &self.resource
    }

    /// Returns the retained temporary resource mutably.
    ///
    /// # Returns
    /// Mutable reference to the resource retained after failure.
    #[inline(always)]
    pub const fn resource_mut(&mut self) -> &mut T {
        &mut self.resource
    }

    /// Returns the target path supplied by the caller.
    ///
    /// # Returns
    /// Requested target before absolute-path resolution.
    #[inline(always)]
    pub fn requested_target(&self) -> &Path {
        &self.requested_target
    }

    /// Returns the resolved absolute target, when resolution succeeded.
    ///
    /// # Returns
    /// Resolved target for parent preparation and destination installation.
    #[inline(always)]
    pub fn resolved_target(&self) -> Option<&Path> {
        self.resolved_target.as_deref()
    }

    /// Returns the stage at which persistence failed.
    ///
    /// # Returns
    /// Failed persistence stage.
    #[inline(always)]
    pub const fn stage(&self) -> LocalPersistStage {
        self.stage
    }

    /// Returns the native I/O error kind.
    ///
    /// # Returns
    /// Error kind reported by the retained native error.
    #[inline(always)]
    pub fn kind(&self) -> io::ErrorKind {
        self.error.kind()
    }

    /// Splits this error into all retained values.
    ///
    /// # Returns
    /// Native error, retained resource, requested target, resolved target, and
    /// failure stage.
    #[inline(always)]
    pub fn into_parts(
        self,
    ) -> (io::Error, T, PathBuf, Option<PathBuf>, LocalPersistStage) {
        let Self {
            error,
            resource,
            requested_target,
            resolved_target,
            stage,
        } = self;
        (error, *resource, requested_target, resolved_target, stage)
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
    /// Returns the retained native I/O error.
    #[inline(always)]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}
