// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Stable classification of a conditional commit failure.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalConditionalFailureReason {
    /// The target content did not match the requested digest.
    PreconditionMismatch,
    /// The expected target was missing.
    TargetMissing,
    /// A target existed when absence was required.
    TargetExists,
    /// The write options cannot be used with the selected condition.
    InvalidOptions,
    /// The requested operation is unsupported by the backend.
    Unsupported,
    /// The writer or backend was in an invalid state.
    InvalidState,
    /// A native I/O operation failed.
    Io,
    /// A conflicting change was observed after publication began.
    PostPublicationConflict,
}
