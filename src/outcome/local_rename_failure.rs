// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Typed failures from unified rename operations.
// qubit-style: allow coverage-cfg

use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::path::Path;

use super::LocalRenameFailureState;
use crate::LocalFileError;

/// Failure details retained when a unified rename does not complete.
#[derive(Debug)]
pub struct LocalRenameFailure {
    /// Primary typed filesystem error.
    error: Box<LocalFileError>,
    /// Most precise namespace state proven by native operations.
    state: LocalRenameFailureState,
}

impl Display for LocalRenameFailure {
    /// Formats the primary rename failure and its proven namespace state.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "rename failed with {:?} state: {}", self.state, self.error)
    }
}

impl Error for LocalRenameFailure {
    /// Returns the primary typed filesystem error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.error.as_ref())
    }
}

impl LocalRenameFailure {
    /// Creates a typed rename failure from implementation facts.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) fn new(error: LocalFileError, state: LocalRenameFailureState) -> Self {
        Self {
            error: Box::new(error),
            state,
        }
    }

    /// Returns the primary typed filesystem error.
    #[must_use]
    pub fn error(&self) -> &LocalFileError {
        self.error.as_ref()
    }

    /// Returns the most precise namespace state proven by native operations.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn state(&self) -> LocalRenameFailureState {
        self.state
    }

    /// Consumes this failure and returns its error and proven state.
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn into_parts(self) -> (LocalFileError, LocalRenameFailureState) {
        (*self.error, self.state)
    }

    /// Rewrites backend path context into normalized public operands.
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
pub type LocalRenameResult = Result<super::LocalRenameOutcome, LocalRenameFailure>;
