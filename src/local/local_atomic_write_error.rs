// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic-write errors.
// qubit-style: allow source-test-pair
// qubit-style: allow inline-tests
// qubit-style: allow explicit-imports

use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteStage;

/// Error returned by an atomic whole-file replacement.
///
/// [`Self::destination_state`] is the authoritative recovery signal. Staging
/// cleanup follows the independently tracked staging-name state. When a
/// destination has already been published, cleanup and parent synchronization
/// failures remain available without replacing the primary installation error.
#[non_exhaustive]
#[derive(Debug)]
pub(crate) struct LocalAtomicWriteError {
    /// Stage at which the operation failed.
    stage: LocalAtomicWriteStage,
    /// Requested destination path.
    path: PathBuf,
    /// Same-directory temporary path when one had already been created.
    temporary_path: Option<PathBuf>,
    /// Known destination state after the failure.
    destination_state: LocalAtomicDestinationState,
    /// Secondary error reported while removing an uncommitted staging file.
    ///
    /// The primary operation error remains available through
    /// [`Self::source_error`] and the [`Error`] implementation.
    cleanup_error: Option<io::Error>,
    /// Secondary error reported while synchronizing a published destination's
    /// parent chain after an installation failure.
    parent_sync_error: Option<io::Error>,
    /// Native I/O error that caused the failure.
    source: io::Error,
}

#[allow(dead_code)]
impl LocalAtomicWriteError {
    /// Creates an atomic-write error.
    ///
    /// # Parameters
    /// - `stage`: Stage at which the operation failed.
    /// - `path`: Requested destination path.
    /// - `temporary_path`: Optional same-directory temporary path.
    /// - `destination_state`: Known destination state after the failure.
    /// - `source`: Native I/O error that caused the failure.
    ///
    /// # Returns
    /// New atomic-write error retaining the native source error.
    #[inline(always)]
    pub(crate) fn new(
        stage: LocalAtomicWriteStage,
        path: PathBuf,
        temporary_path: Option<PathBuf>,
        destination_state: LocalAtomicDestinationState,
        source: io::Error,
    ) -> Self {
        Self {
            stage,
            path,
            temporary_path,
            destination_state,
            cleanup_error: None,
            parent_sync_error: None,
            source,
        }
    }

    /// Returns the stage at which the operation failed.
    ///
    /// # Returns
    /// Failed atomic-write stage.
    pub(crate) const fn stage(&self) -> LocalAtomicWriteStage {
        self.stage
    }

    /// Returns the requested destination path.
    ///
    /// # Returns
    /// Destination path supplied by the caller.
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the same-directory staging path, when one was created.
    ///
    /// # Returns
    /// Staging path retained for diagnostics. The entry is not guaranteed to
    /// exist after a completed replacement or a successful cleanup.
    pub(crate) fn temporary_path(&self) -> Option<&Path> {
        self.temporary_path.as_deref()
    }

    /// Returns the known destination state after the failure.
    ///
    /// # Returns
    /// State reported by the failed operation. Callers must handle
    /// [`LocalAtomicDestinationState::Indeterminate`] conservatively and
    /// inspect the destination and staging path before retrying.
    pub(crate) const fn destination_state(
        &self,
    ) -> LocalAtomicDestinationState {
        self.destination_state
    }

    /// Returns the secondary staging cleanup error, when cleanup failed.
    ///
    /// # Returns
    /// Cleanup error without replacing the primary source error.
    pub(crate) fn cleanup_error(&self) -> Option<&io::Error> {
        self.cleanup_error.as_ref()
    }

    /// Returns the secondary parent synchronization error, when synchronization
    /// failed after the destination had already been published.
    ///
    /// # Returns
    /// Parent synchronization error without replacing the primary source error.
    pub(crate) fn parent_sync_error(&self) -> Option<&io::Error> {
        self.parent_sync_error.as_ref()
    }

    /// Returns the native I/O error that caused the atomic write to fail.
    ///
    /// # Returns
    /// Retained primary I/O error without dynamic downcasting.
    #[must_use]
    pub(crate) const fn source_error(&self) -> &io::Error {
        &self.source
    }

    /// Returns the native I/O error kind.
    ///
    /// # Returns
    /// Error kind reported by the retained source error.
    #[must_use]
    pub(crate) fn kind(&self) -> io::ErrorKind {
        self.source.kind()
    }

    /// Consumes this error and returns staging cleanup details with its source.
    #[inline]
    pub(crate) fn into_staging_parts(
        self,
    ) -> (Option<PathBuf>, Option<io::Error>, io::Error) {
        (self.temporary_path, self.cleanup_error, self.source)
    }

    /// Attaches a staging cleanup failure.
    ///
    /// # Parameters
    /// - `cleanup_error`: Secondary error raised while removing the staging
    ///   path.
    ///
    /// # Returns
    /// This atomic-write error enriched with cleanup context.
    #[inline]
    pub(crate) fn with_cleanup_error(
        mut self,
        cleanup_error: Option<io::Error>,
    ) -> Self {
        self.cleanup_error = cleanup_error;
        self
    }

    /// Attaches a parent synchronization failure.
    ///
    /// # Parameters
    /// - `parent_sync_error`: Secondary error raised while synchronizing a
    ///   published destination's parent chain.
    ///
    /// # Returns
    /// This atomic-write error enriched with parent synchronization context.
    #[inline]
    pub(crate) fn with_parent_sync_error(
        mut self,
        parent_sync_error: Option<io::Error>,
    ) -> Self {
        self.parent_sync_error = parent_sync_error;
        self
    }
}

impl Display for LocalAtomicWriteError {
    /// Formats the failed stage, destination state, and source error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "atomic write to '{}' failed during {:?} \
             (destination_state={:?}): {}",
            self.path.display(),
            self.stage,
            self.destination_state,
            self.source
        )?;
        if let Some(temporary_path) = self.temporary_path.as_ref() {
            write!(formatter, "; staging path '{}'", temporary_path.display())?;
        }
        match (self.cleanup_error.as_ref(), self.parent_sync_error.as_ref()) {
            (Some(cleanup_error), Some(parent_sync_error)) => write!(
                formatter,
                "; staging cleanup also failed: {cleanup_error}; parent \
                 synchronization also failed: {parent_sync_error}",
            ),
            (Some(cleanup_error), None) => {
                write!(
                    formatter,
                    "; staging cleanup also failed: {cleanup_error}",
                )
            }
            (None, Some(parent_sync_error)) => write!(
                formatter,
                "; parent synchronization also failed: {parent_sync_error}",
            ),
            (None, None) => Ok(()),
        }
    }
}

impl Error for LocalAtomicWriteError {
    /// Returns the retained native I/O error.
    #[inline(always)]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source_error())
    }
}

// This module tests private atomic-writer failure decomposition and retry
// ownership. The public writer API cannot manufacture each internal failure
// state, and a test hook would expose unstable state-machine details. Public
// writer integration tests cover the resulting retry and terminal behavior.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalAtomicDestinationState;
    use crate::LocalAtomicWriteStage;

    fn error() -> LocalAtomicWriteError {
        LocalAtomicWriteError::new(
            LocalAtomicWriteStage::ReplaceDestination,
            "target".into(),
            Some("staging".into()),
            LocalAtomicDestinationState::Replaced,
            io::Error::other("boom"),
        )
    }

    #[test]
    fn exposes_context_and_formats_secondary_errors() {
        let error = error()
            .with_cleanup_error(Some(io::Error::other("cleanup")))
            .with_parent_sync_error(Some(io::Error::other("sync")));
        assert_eq!(error.stage(), LocalAtomicWriteStage::ReplaceDestination);
        assert_eq!(error.path(), Path::new("target"));
        assert_eq!(error.temporary_path(), Some(Path::new("staging")));
        assert_eq!(
            error.destination_state(),
            LocalAtomicDestinationState::Replaced
        );
        assert!(error.cleanup_error().is_some());
        assert!(error.parent_sync_error().is_some());
        assert_eq!(error.source_error().kind(), io::ErrorKind::Other);
        assert_eq!(error.kind(), io::ErrorKind::Other);
        let display = error.to_string();
        assert!(display.contains("staging cleanup"));
        assert!(display.contains("parent synchronization"));
        assert!(error.source().is_some());
    }

    #[test]
    fn splits_staging_parts_without_optional_context() {
        let (path, cleanup, source) = error().into_staging_parts();
        assert_eq!(path, Some("staging".into()));
        assert!(cleanup.is_none());
        assert_eq!(source.kind(), io::ErrorKind::Other);
        let no_staging = LocalAtomicWriteError::new(
            LocalAtomicWriteStage::PrepareParent,
            "target".into(),
            None,
            LocalAtomicDestinationState::Unchanged,
            io::Error::other("boom"),
        );
        assert!(!no_staging.to_string().contains("staging path"));
        let cleanup_only =
            error().with_cleanup_error(Some(io::Error::other("cleanup")));
        assert!(cleanup_only.to_string().contains("staging cleanup"));
        let parent_only =
            error().with_parent_sync_error(Some(io::Error::other("sync")));
        assert!(parent_only.to_string().contains("parent synchronization"));
        let plain = error()
            .with_cleanup_error(None)
            .with_parent_sync_error(None);
        assert!(plain.to_string().contains("atomic write"));
    }
}
