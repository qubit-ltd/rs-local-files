// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use std::{error::Error, fmt};

use crate::{LocalFileError, LocalFileWriter, LocalWriterState};

/// Failed writer commit with publication state and an optional retryable
/// writer.
#[derive(Debug)]
pub struct LocalFileCommitError {
    /// Structured local filesystem failure.
    error: LocalFileError,
    /// Publication state established by the failed attempt.
    state: LocalWriterState,
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
    #[inline(always)]
    pub(crate) fn new(
        error: LocalFileError,
        state: LocalWriterState,
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
    #[inline(always)]
    pub const fn error(&self) -> &LocalFileError {
        &self.error
    }

    /// Returns the established publication state.
    #[must_use]
    #[inline(always)]
    pub const fn state(&self) -> LocalWriterState {
        self.state
    }

    /// Returns a retryable writer, or `None` after publication may have
    /// started.
    #[must_use]
    #[inline(always)]
    pub fn writer(&self) -> Option<&LocalFileWriter> {
        self.writer.as_deref()
    }

    /// Consumes the failure into its error, state, and optional retryable
    /// writer.
    #[must_use]
    #[inline(always)]
    pub fn into_parts(self) -> (LocalFileError, LocalWriterState, Option<LocalFileWriter>) {
        (self.error, self.state, self.writer.map(|writer| *writer))
    }
}

impl fmt::Display for LocalFileCommitError {
    /// Formats the structured failure and established publication state.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({:?})", self.error, self.state)
    }
}

impl Error for LocalFileCommitError {
    /// Returns the structured local filesystem failure.
    #[inline(always)]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}
