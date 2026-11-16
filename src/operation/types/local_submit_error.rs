// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalSubmitRejection;

/// Preserves the rejected request so the caller can recover its ownership.
/// # Type Parameters
/// - `R` — owned request returned after admission is rejected.

pub struct LocalSubmitError<R> {
    /// Stores the request associated with this value.
    request: R,
    /// Stores the rejection associated with this value.
    rejection: LocalSubmitRejection,
}

impl<R> LocalSubmitError<R> {
    /// Runs the new behavior while preserving the operation lifecycle
    /// invariants.

    /// # Parameters
    /// - `rejection` — supplies data or policy for this operation.
    #[allow(dead_code)]
    pub(crate) fn new(request: R, rejection: LocalSubmitRejection) -> Self {
        Self { request, rejection }
    }

    /// Returns the original request without requiring it to implement Clone or
    /// Debug.
    pub fn into_request(self) -> R {
        self.request
    }

    /// Returns the admission reason.
    pub fn rejection(&self) -> LocalSubmitRejection {
        self.rejection
    }
}

impl<R> std::fmt::Debug for LocalSubmitError<R> {
    /// Runs the fmt behavior while preserving the operation lifecycle
    /// invariants.

    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalSubmitError")
            .field("rejection", &self.rejection)
            .finish_non_exhaustive()
    }
}

impl<R> std::fmt::Display for LocalSubmitError<R> {
    /// Runs the fmt behavior while preserving the operation lifecycle
    /// invariants.

    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "local operation rejected: {:?}", self.rejection)
    }
}

impl<R: 'static> std::error::Error for LocalSubmitError<R> {}
