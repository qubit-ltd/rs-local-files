// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use std::{
    error::Error,
    fmt,
};

use crate::{
    LocalFileError,
    LocalFileWriter,
    LocalWriteFailureState,
};

/// Failed writer commit with publication state and an optional retryable
/// writer.
#[derive(Debug)]
pub struct LocalFileCommitError {
    /// Structured local filesystem failure.
    error: LocalFileError,
    /// Publication state established by the failed attempt.
    state: LocalWriteFailureState,
    /// Writer retained only when retry or explicit abort remains safe.
    writer: Option<Box<LocalFileWriter>>,
}

impl LocalFileCommitError {
    /// Creates a commit failure.
    ///
    /// # Parameters
    ///
    /// - `error`: Structured local filesystem failure.
    /// - `state`: Established publication state.
    /// - `writer`: Retryable writer when publication has not started.
    #[inline]
    pub(crate) fn new(
        error: LocalFileError,
        state: LocalWriteFailureState,
        writer: Option<LocalFileWriter>,
    ) -> Self {
        Self {
            error,
            state,
            writer: writer.map(Box::new),
        }
    }

    /// Returns the structured local filesystem failure.
    #[must_use]
    pub const fn error(&self) -> &LocalFileError {
        &self.error
    }

    /// Returns the established publication state.
    pub const fn state(&self) -> LocalWriteFailureState {
        self.state
    }

    /// Returns a retryable writer, or `None` after publication may have
    /// started.
    #[must_use]
    pub fn writer(&self) -> Option<&LocalFileWriter> {
        self.writer.as_deref()
    }

    /// Consumes the failure into its error, state, and optional retryable
    /// writer.
    pub fn into_parts(
        self,
    ) -> (
        LocalFileError,
        LocalWriteFailureState,
        Option<LocalFileWriter>,
    ) {
        (self.error, self.state, self.writer.map(|writer| *writer))
    }
}

impl fmt::Display for LocalFileCommitError {
    /// Formats the structured failure and established publication state.
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({:?})", self.error, self.state)
    }
}

impl Error for LocalFileCommitError {
    /// Returns the structured local filesystem failure.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}
