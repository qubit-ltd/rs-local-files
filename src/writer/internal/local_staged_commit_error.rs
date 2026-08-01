// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Recoverable staged-writer commit error.
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use crate::local::LocalAtomicWriteError;

use super::LocalFileWriterBackend;

/// Recoverable staged-publication failure with its optional retained backend.
#[derive(Debug)]
pub(crate) struct LocalStagedCommitError {
    /// Structured failure reported by the selected atomic writer.
    pub(crate) error: LocalAtomicWriteError,
    /// Staged backend retained before publication began.
    pub(crate) backend: Option<Box<LocalFileWriterBackend>>,
}

impl LocalStagedCommitError {
    /// Splits the failure into its error and optional retryable backend.
    #[must_use]
    #[inline]
    pub(crate) fn into_parts(self) -> (LocalAtomicWriteError, Option<LocalFileWriterBackend>) {
        (self.error, self.backend.map(|backend| *backend))
    }
}
