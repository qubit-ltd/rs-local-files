// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Typed failures from unified rename operations.

use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::path::Path;

use super::LocalRenameFailureState;
use super::LocalRenameOutcome;
use crate::LocalFileError;

/// Failure details retained when a unified rename does not complete.
///
/// Inspect the primary error and proven namespace state before deciding
/// whether a rename can be retried.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::outcome::LocalRenameFailureState;
///
/// let filesystem = LocalFileSystem::host()?;
/// let directory = tempfile::tempdir()?;
/// let source = directory.path().join("missing-source");
/// let target = directory.path().join("rename-target");
/// let failure = filesystem
///     .rename(Path::new(&source), Path::new(&target))
///     .expect_err("source is missing");
/// assert_eq!(failure.state(), LocalRenameFailureState::Unchanged);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct LocalRenameFailure {
    /// Primary typed filesystem error.
    error: Box<LocalFileError>,
    /// Most precise namespace state proven by native operations.
    state: LocalRenameFailureState,
}

impl Display for LocalRenameFailure {
    /// Formats the primary rename failure and its proven namespace state.
    ///
    /// # Parameters
    ///
    /// - `formatter`: Destination formatter receiving the failure description.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the failure description is written successfully.
    ///
    /// # Errors
    ///
    /// Returns the formatter's error if writing the description fails.
    #[inline]
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "rename failed with {:?} state: {}", self.state, self.error)
    }
}

impl Error for LocalRenameFailure {
    /// Returns the primary typed filesystem error.
    ///
    /// # Returns
    ///
    /// `Some` containing the underlying [`LocalFileError`]. This implementation
    /// always returns `Some` because the failure retains its primary error;
    /// `None` is not returned.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.error.as_ref())
    }
}

impl LocalRenameFailure {
    /// Creates a typed rename failure from implementation facts.
    ///
    /// # Parameters
    ///
    /// - `error`: Primary typed filesystem error to retain.
    /// - `state`: Namespace state proven by the failed rename operation.
    ///
    /// # Returns
    ///
    /// A failure that owns `error` and records `state`.
    #[must_use = "handle the constructed rename failure"]
    #[inline]
    pub(crate) fn new(error: LocalFileError, state: LocalRenameFailureState) -> Self {
        Self {
            error: Box::new(error),
            state,
        }
    }

    /// Returns the primary typed filesystem error.
    ///
    /// # Returns
    ///
    /// A shared reference to the error retained by this failure.
    #[must_use = "inspect the primary rename error"]
    pub fn error(&self) -> &LocalFileError {
        self.error.as_ref()
    }

    /// Returns the most precise namespace state proven by native operations.
    ///
    /// # Returns
    ///
    /// The namespace state established by the native rename operations.
    #[must_use = "inspect the proven rename failure state"]
    #[inline]
    pub const fn state(&self) -> LocalRenameFailureState {
        self.state
    }

    /// Consumes this failure and returns its error and proven state.
    ///
    /// # Returns
    ///
    /// The owned primary error and its proven namespace state.
    #[must_use = "inspect the primary rename error and proven state"]
    #[inline]
    pub fn into_parts(self) -> (LocalFileError, LocalRenameFailureState) {
        (*self.error, self.state)
    }

    /// Rewrites backend path context into normalized public operands.
    ///
    /// # Parameters
    ///
    /// - `source`: Normalized source operand path.
    /// - `target`: Normalized destination operand path.
    /// - `current_directory`: Namespace-absolute working directory used to
    ///   resolve relative operands, or `None` when no directory is available.
    ///
    /// # Returns
    ///
    /// This failure with its error paths rewritten to the supplied operands.
    pub(crate) fn remap_namespace(mut self, source: &Path, target: &Path, current_directory: Option<&Path>) -> Self {
        self.error.replace_paths(
            Some(source.to_path_buf()),
            Some(target.to_path_buf()),
            current_directory.map(Path::to_path_buf),
        );
        self
    }
}

/// Result returned by unified rename operations.
pub type LocalRenameResult = Result<LocalRenameOutcome, LocalRenameFailure>;
