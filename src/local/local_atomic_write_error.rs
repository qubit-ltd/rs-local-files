// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic-write errors.

use std::error::Error;
use std::fmt::{
    Display,
    Formatter,
    Result as FmtResult,
};
use std::io;
use std::path::{
    Path,
    PathBuf,
};

use crate::{
    LocalAtomicDestinationState,
    LocalAtomicWriteStage,
};

/// Error returned by an atomic whole-file replacement.
///
/// [`Self::destination_state`] is the authoritative recovery signal. Staging
/// cleanup is attempted only for [`LocalAtomicDestinationState::Unchanged`].
/// Other states retain any still-existing staging entry because cleanup could
/// destroy recovery evidence; the path may already have been moved when the
/// destination is [`LocalAtomicDestinationState::Replaced`].
#[non_exhaustive]
#[derive(Debug)]
pub struct LocalAtomicWriteError {
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
    /// The primary operation error remains available through [`Self::source`]
    /// and the [`Error`] implementation.
    cleanup_error: Option<io::Error>,
    /// Native I/O error that caused the failure.
    source: io::Error,
}

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
    #[inline]
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
            source,
        }
    }

    /// Returns the stage at which the operation failed.
    ///
    /// # Returns
    /// Failed atomic-write stage.
    #[inline(always)]
    pub const fn stage(&self) -> LocalAtomicWriteStage {
        self.stage
    }

    /// Returns the requested destination path.
    ///
    /// # Returns
    /// Destination path supplied by the caller.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the same-directory staging path, when one was created.
    ///
    /// # Returns
    /// Staging path retained for diagnostics. The entry is not guaranteed to
    /// exist after a completed replacement or a successful cleanup.
    #[inline(always)]
    pub fn temporary_path(&self) -> Option<&Path> {
        self.temporary_path.as_deref()
    }

    /// Returns the known destination state after the failure.
    ///
    /// # Returns
    /// State reported by the failed operation. Callers must handle
    /// [`LocalAtomicDestinationState::Indeterminate`] conservatively and
    /// inspect the destination and staging path before retrying.
    #[inline(always)]
    pub const fn destination_state(&self) -> LocalAtomicDestinationState {
        self.destination_state
    }

    /// Returns the secondary staging cleanup error, when cleanup failed.
    ///
    /// # Returns
    /// Cleanup error without replacing the primary source error.
    #[inline(always)]
    pub fn cleanup_error(&self) -> Option<&io::Error> {
        self.cleanup_error.as_ref()
    }

    /// Returns the native I/O error kind.
    ///
    /// # Returns
    /// Error kind reported by the retained source error.
    #[must_use]
    #[inline(always)]
    pub fn kind(&self) -> io::ErrorKind {
        self.source.kind()
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
        if let Some(cleanup_error) = self.cleanup_error.as_ref() {
            return write!(
                formatter,
                "; staging cleanup also failed: {cleanup_error}"
            );
        }
        Ok(())
    }
}

impl Error for LocalAtomicWriteError {
    /// Returns the retained native I/O error.
    #[inline(always)]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}
