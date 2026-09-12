// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recoverable temporary-resource persistence errors.
// qubit-style: allow source-test-pair
// qubit-style: allow explicit-imports

use std::error::Error;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalPersistFailureState;
use crate::LocalPersistStage;
use crate::local::LocalPersistErrorParts;
use crate::outcome::LocalTempSourceState;

/// Persistence error that returns ownership of the temporary resource.
///
/// The stage distinguishes target resolution, parent preparation, source
/// synchronization, final installation, and destination synchronization.
/// [`Self::requested_target`] always returns the caller's path;
/// [`Self::resolved_target`] returns the bound absolute path once resolution
/// has succeeded. The resource remains available for retry, inspection, keep,
/// or explicit cleanup only while the retained resource still has a known
/// owned namespace entry. A `Published` failure retains only residual sandbox
/// cleanup and never removes the destination. After an indeterminate native
/// publish failure, temporary handles reject cleanup and their `Drop`
/// implementation performs no namespace operation.
///
/// # Examples
///
/// A no-replace conflict leaves the destination unchanged and returns an owned
/// temporary resource that can be cleaned up explicitly:
///
/// ```
/// use std::io::Write;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalTempDirectoryOptions;
/// use qubit_local_files::options::LocalTempFileOptions;
/// use qubit_local_files::outcome::LocalPersistFailureState;
/// use qubit_local_files::outcome::LocalTempSourceState;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let parent_options = LocalTempDirectoryOptions::new()
///     .with_parent(&std::env::temp_dir())
///     .with_max_attempts(16);
/// let mut parent = filesystem.create_temp_directory_with_options(&parent_options)?;
/// let target = parent.path().join("manifest.json");
/// std::fs::write(&target, b"existing manifest")?;
/// let options = LocalTempFileOptions::new().with_parent(parent.path());
/// let mut temporary = filesystem.create_temp_file_with_options(&options)?;
/// temporary.write_all(br#"{"complete":true}"#)?;
///
/// let failure = temporary
///     .persist(&target)
///     .expect_err("no-replace must reject an existing target");
/// assert_eq!(failure.state(), LocalPersistFailureState::NotPublished);
/// assert_eq!(failure.source_state(), LocalTempSourceState::Owned);
/// let mut parts = failure.into_parts();
/// assert_eq!(parts.state, LocalPersistFailureState::NotPublished);
/// assert_eq!(parts.source_state, LocalTempSourceState::Owned);
/// parts.resource.cleanup()?;
/// assert_eq!(parts.resource.source_state(), LocalTempSourceState::Released);
/// assert_eq!(std::fs::read(&target)?, b"existing manifest");
/// parent.cleanup()?;
/// # Ok(())
/// # }
/// ```
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
    /// Source authority snapshot at failure construction.
    source_state: LocalTempSourceState,
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
    /// - `state`: Publication fact explicitly established by this call.
    /// - `source_state`: Current source authority snapshot.
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
        state: LocalPersistFailureState,
        source_state: LocalTempSourceState,
    ) -> Self {
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
            source_state,
        }
    }

    /// Returns the structured persistence error.
    ///
    /// # Returns
    /// Structured error that prevented persistence.
    #[must_use = "the structured persistence error should be inspected"]
    #[inline]
    pub const fn error(&self) -> &LocalFileError {
        &self.error
    }

    /// Returns the retained temporary resource.
    ///
    /// # Returns
    /// Shared reference to the resource retained after failure.
    #[must_use]
    #[inline]
    pub const fn resource(&self) -> &T {
        &self.resource
    }

    /// Returns the retained temporary resource mutably.
    ///
    /// # Returns
    /// Mutable reference to the resource retained after failure.
    #[must_use]
    #[inline]
    pub const fn resource_mut(&mut self) -> &mut T {
        &mut self.resource
    }

    /// Returns the target path supplied by the caller.
    ///
    /// # Returns
    /// Requested target before absolute-path resolution.
    #[must_use]
    #[inline]
    pub fn requested_target(&self) -> &Path {
        &self.requested_target
    }

    /// Returns the resolved absolute target, when resolution succeeded.
    ///
    /// # Returns
    /// Resolved target for parent preparation and destination installation.
    #[must_use]
    #[inline]
    pub fn resolved_target(&self) -> Option<&Path> {
        self.resolved_target.as_deref()
    }

    /// Returns the stage at which persistence failed.
    ///
    /// # Returns
    /// Failed persistence stage.
    #[must_use = "the failed persistence stage should be inspected"]
    #[inline]
    pub const fn stage(&self) -> LocalPersistStage {
        self.stage
    }

    /// Returns the publication fact established by this call.
    ///
    /// # Returns
    /// Whether this call published its target; source authority is independent.
    #[must_use = "inspect the retained persistence state"]
    #[inline]
    pub const fn state(&self) -> LocalPersistFailureState {
        self.state
    }

    /// Returns the stable persistence error kind.
    ///
    /// # Returns
    /// Stable classification reported by the retained structured error.
    #[must_use = "inspect the retained persistence error kind"]
    #[inline]
    pub const fn kind(&self) -> crate::LocalFileErrorKind {
        self.error.kind()
    }

    /// Returns the source authority snapshot captured at failure.
    ///
    /// After `resource_mut()` changes the resource, query that resource's
    /// `source_state()` for its current authority.
    #[must_use = "inspect the source authority before choosing a recovery action"]
    #[inline]
    pub const fn source_state(&self) -> LocalTempSourceState {
        self.source_state
    }

    /// Splits this error into named fields, retaining both recovery axes.
    #[must_use = "the returned parts retain the resource and recovery context"]
    pub fn into_parts(self) -> LocalPersistErrorParts<T> {
        LocalPersistErrorParts {
            error: *self.error,
            resource: *self.resource,
            requested_target: self.requested_target,
            resolved_target: self.resolved_target,
            stage: self.stage,
            state: self.state,
            source_state: self.source_state,
        }
    }

    /// Attaches the retained resource to an already-captured failure snapshot.
    pub(crate) fn with_resource<U>(self, resource: U) -> LocalPersistError<U> {
        LocalPersistError {
            error: self.error,
            resource: Box::new(resource),
            requested_target: self.requested_target,
            resolved_target: self.resolved_target,
            stage: self.stage,
            state: self.state,
            source_state: self.source_state,
        }
    }

    /// Attaches the PWD snapshot retained by the temporary resource.
    pub(crate) fn with_current_directory(mut self, current_directory: PathBuf) -> Self {
        self.error = Box::new((*self.error).with_current_directory(current_directory));
        self
    }

    /// Reclassifies a constructed persistence failure with a stable public
    /// error kind while retaining its native source details.
    pub(crate) fn with_kind(mut self, kind: LocalFileErrorKind) -> Self {
        self.error = Box::new((*self.error).with_kind(kind));
        self
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

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;
    use std::path::Path;
    use std::path::PathBuf;

    use super::LocalPersistError;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalPersistFailureState;
    use crate::LocalPersistStage;
    use crate::outcome::LocalTempSourceState;

    #[test]
    fn test_persist_error_exposes_recoverable_context_and_resource() {
        let mut error = LocalPersistError::new(
            io::Error::from(io::ErrorKind::NotFound),
            String::from("temporary"),
            PathBuf::from("requested"),
            Some(PathBuf::from("/resolved")),
            LocalPersistStage::PrepareParent,
            LocalPersistFailureState::NotPublished,
            LocalTempSourceState::Owned,
        )
        .with_current_directory(PathBuf::from("/workspace"));

        assert_eq!(LocalFileErrorKind::NotFound, error.kind());
        assert_eq!(LocalPersistStage::PrepareParent, error.stage());
        assert_eq!(LocalPersistFailureState::NotPublished, error.state());
        assert_eq!(Path::new("requested"), error.requested_target());
        assert_eq!(Some(Path::new("/resolved")), error.resolved_target());
        assert_eq!("temporary", error.resource());
        error.resource_mut().push_str("-updated");
        assert_eq!("temporary-updated", error.resource());
        assert!(error.to_string().contains("resolved as '/resolved'"));
        assert!(Error::source(&error).is_some());
        assert_eq!(Some(Path::new("/workspace")), error.error().current_directory());
    }

    #[test]
    fn test_persist_error_parts_preserve_indeterminate_install_state() {
        let error = LocalPersistError::new(
            io::Error::from(io::ErrorKind::PermissionDenied),
            7_u8,
            PathBuf::from("requested"),
            None,
            LocalPersistStage::InstallDestination,
            LocalPersistFailureState::Indeterminate,
            LocalTempSourceState::Indeterminate,
        );

        let crate::local::LocalPersistErrorParts {
            error: source,
            resource,
            requested_target: requested,
            resolved_target: resolved,
            stage,
            state,
            ..
        } = error.into_parts();
        assert_eq!(io::ErrorKind::PermissionDenied, source.io_error_kind());
        assert_eq!(7, resource);
        assert_eq!(PathBuf::from("requested"), requested);
        assert_eq!(None, resolved);
        assert_eq!(LocalPersistStage::InstallDestination, stage);
        assert_eq!(LocalPersistFailureState::Indeterminate, state);
        assert_eq!(LocalFileOperation::PersistTemp, source.operation());
    }

    #[test]
    fn test_persist_error_retains_explicit_durability_failure_states() {
        let source_error = LocalPersistError::new(
            io::Error::from(io::ErrorKind::Other),
            (),
            PathBuf::from("requested"),
            Some(PathBuf::from("/resolved")),
            LocalPersistStage::SynchronizeSource,
            LocalPersistFailureState::NotPublished,
            LocalTempSourceState::Owned,
        );
        assert_eq!(LocalPersistFailureState::NotPublished, source_error.state());

        let destination_error = LocalPersistError::new(
            io::Error::from(io::ErrorKind::Other),
            (),
            PathBuf::from("requested"),
            Some(PathBuf::from("/resolved")),
            LocalPersistStage::SynchronizeDestination,
            LocalPersistFailureState::Published,
            LocalTempSourceState::CleanupRequired,
        );
        assert_eq!(LocalPersistFailureState::Published, destination_error.state());
    }
}
