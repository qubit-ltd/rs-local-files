// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

/// Namespace state established by a failed writer publication attempt.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalWriteFailureState {
    /// The destination was proven unchanged.
    NotPublished,
    /// The destination changed before a later failure.
    Published,
    /// The final destination state cannot be determined safely.
    Indeterminate,
}
