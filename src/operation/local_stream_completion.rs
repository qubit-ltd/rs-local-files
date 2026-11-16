// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Result returned by the serial worker.
///
/// A completion is delivered once for the matching request identifier.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalStreamCompletion;
///
/// let completion = LocalStreamCompletion::Written(12);
/// assert!(matches!(completion, LocalStreamCompletion::Written(12)));
/// ```
#[derive(Debug)]
pub enum LocalStreamCompletion {
    /// Bytes returned by a read command.
    Read(Vec<u8>),
    /// Number of bytes accepted by a write command.
    Written(usize),
    /// Flush completed successfully.
    Flushed,
    /// New absolute position after a seek.
    Seeked(u64),
    /// Writer publication completed.
    Committed(crate::LocalWriteOutcome),
    /// Publication failed, with the writer retained when retry is possible.
    CommitFailed {
        /// Structured reason publication failed.
        error: crate::LocalFileCommitError,
        /// Whether the worker still owns a retryable writer.
        retryable: bool,
    },
    /// Native stream closure was confirmed.
    Closed,
    /// Worker/driver disappeared after command submission, so its side effect
    /// cannot be confirmed.
    Unconfirmed {
        operation: &'static str,
        detail: String,
    },
}
