// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Failure to admit a stream stage while preserving its owned input.
///
/// The generic input is returned on either failure path so ownership is never
/// lost when the operation is rejected before execution.
///
/// # Type Parameters
///
/// * `I` — owned input stream type supplied by the caller.
pub enum LocalStageStreamSubmitError<I> {
    /// A configured input or disk budget was exceeded before admission.
    Budget {
        /// Original input returned to the caller.
        input: I,
    },
    /// The operation manager rejected admission.
    Admission {
        /// Manager's admission rejection reason.
        reason: qubit_operation::AdmissionError,
        /// Original input returned to the caller.
        input: I,
    },
}

impl<I> std::fmt::Debug for LocalStageStreamSubmitError<I> {
    /// Formats rejection diagnostics without printing or polling the owned
    /// input.
    ///
    /// # Parameters
    ///
    /// * `self` — rejected input wrapper.
    /// * `formatter` — destination for the formatted rejection.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget { .. } => formatter.write_str("LocalStageStreamSubmitError::Budget { input: <owned> }"),
            Self::Admission { reason, .. } => formatter
                .debug_struct("LocalStageStreamSubmitError::Admission")
                .field("reason", reason)
                .field("input", &"<owned>")
                .finish(),
        }
    }
}
