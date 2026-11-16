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
    Read(#[doc = "Bytes returned by the worker, possibly fewer than requested at EOF."] Vec<u8>),
    /// Number of bytes accepted by a write command.
    Written(#[doc = "Number of input bytes accepted by the worker."] usize),
    /// Flush completed successfully.
    Flushed,
    /// New absolute position after a seek.
    Seeked(#[doc = "Absolute position reached after the seek."] u64),
    /// Writer publication completed.
    Committed(
        #[doc = "Publication and durability facts confirmed by commit."] crate::LocalWriteOutcome,
    ),
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
        /// Operation whose native result could not be observed.
        operation: &'static str,
        /// Diagnostic detail explaining why the worker result is unavailable.
        detail: String,
    },
}
