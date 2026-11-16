// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Rejection before stage admission, preserving the original owned allocation.
#[derive(Debug)]
pub enum LocalStageSubmitError {
    /// Payload or reservation exceeded the stage budget before admission.
    Budget {
        /// Original allocation returned to the caller unchanged.
        bytes: Vec<u8>,
    },
    /// The operation manager rejected admission before taking ownership.
    Admission {
        /// Manager's admission rejection reason.
        reason: qubit_operation::AdmissionError,
        /// Original allocation returned to the caller unchanged.
        bytes: Vec<u8>,
    },
}

impl LocalStageSubmitError {
    /// Returns the original payload after pre-admission rejection.
    ///
    /// # Parameters
    ///
    /// * `self` — rejected request containing the caller's original allocation.
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::Budget { bytes } | Self::Admission { bytes, .. } => bytes,
        }
    }
}
