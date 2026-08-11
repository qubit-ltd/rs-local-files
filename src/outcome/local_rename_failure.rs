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

use super::LocalRenameFailureState;
use crate::LocalFileError;

/// Failure details retained when a unified rename does not complete.
#[derive(Debug)]
pub struct LocalRenameFailure {
    /// Primary typed filesystem error.
    error: LocalFileError,
    /// Most precise namespace state proven by native operations.
    state: LocalRenameFailureState,
}

impl Display for LocalRenameFailure {
    /// Formats the primary rename failure and its proven namespace state.
    #[inline]
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "rename failed with {:?} state: {}",
            self.state, self.error
        )
    }
}

impl Error for LocalRenameFailure {
    /// Returns the primary typed filesystem error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

impl LocalRenameFailure {
    /// Creates a typed rename failure from implementation facts.
    #[must_use]
    pub(crate) const fn new(
        error: LocalFileError,
        state: LocalRenameFailureState,
    ) -> Self {
        Self { error, state }
    }

    /// Returns the primary typed filesystem error.
    #[must_use]
    pub const fn error(&self) -> &LocalFileError {
        &self.error
    }

    /// Returns the most precise namespace state proven by native operations.
    pub const fn state(&self) -> LocalRenameFailureState {
        self.state
    }

    /// Consumes this failure and returns its error and proven state.
    pub fn into_parts(self) -> (LocalFileError, LocalRenameFailureState) {
        (self.error, self.state)
    }
}

/// Result returned by unified rename operations.
pub type LocalRenameResult =
    Result<super::LocalRenameOutcome, LocalRenameFailure>;
