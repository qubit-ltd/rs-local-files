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

#[cfg(test)]
mod tests {
    use super::LocalStageStreamSubmitError;

    #[test]
    fn test_budget_rejection_hides_input_in_debug_and_returns_ownership() {
        let error = LocalStageStreamSubmitError::Budget { input: vec![1, 2, 3] };
        assert_eq!(
            format!("{error:?}"),
            "LocalStageStreamSubmitError::Budget { input: <owned> }"
        );
        let LocalStageStreamSubmitError::Budget { input } = error else {
            panic!("budget failure variant is preserved");
        };
        assert_eq!(input, [1, 2, 3]);
    }

    #[test]
    fn test_admission_rejection_hides_input_and_preserves_reason_and_ownership() {
        let error = LocalStageStreamSubmitError::Admission {
            reason: qubit_operation::AdmissionError::CapacityExceeded,
            input: vec![4, 5],
        };
        let debug = format!("{error:?}");
        assert!(debug.contains("CapacityExceeded"));
        assert!(debug.contains("input: \"<owned>\""));
        let LocalStageStreamSubmitError::Admission { reason, input } = error else {
            panic!("admission failure variant is preserved");
        };
        assert_eq!(reason, qubit_operation::AdmissionError::CapacityExceeded);
        assert_eq!(input, [4, 5]);
    }
}
