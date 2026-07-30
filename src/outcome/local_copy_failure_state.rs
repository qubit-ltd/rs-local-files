// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// qubit-style: allow source-test-pair
// Outcome states are covered through public copy integration tests.
//! States proven when a unified copy operation fails.

/// Namespace state proven by a failed unified copy operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCopyFailureState {
    /// No destination entry was changed.
    Unchanged,
    /// Some destination entries were written, but the full requested content
    /// was not published.
    PartiallyPublished,
    /// The complete destination content was published before a later failure.
    Published,
    /// Native I/O failed without proving the final destination state.
    Indeterminate,
}
