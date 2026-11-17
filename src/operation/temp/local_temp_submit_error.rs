// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Admission failure returned before a temporary-resource operation is
/// accepted.
///
/// The request value is returned intact so the caller can retry with the same
/// options after addressing the admission condition.
///
/// # Type Parameters
///
/// * `R` — request options retained after manager rejection.
#[derive(Debug)]
pub struct LocalTempSubmitError<R> {
    /// Upstream operation admission rejection.
    pub reason: qubit_operation::AdmissionError,
    /// Original request configuration returned for a possible retry.
    pub(super) request: R,
}

impl<R> LocalTempSubmitError<R> {
    /// Returns the original options supplied for the rejected request.
    ///
    /// # Parameters
    ///
    /// * `self` — rejected request wrapper carrying the original options.
    pub fn into_request(self) -> R {
        self.request
    }
}
