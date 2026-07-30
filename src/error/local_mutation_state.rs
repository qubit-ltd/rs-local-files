// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by structured error integration tests.

/// Namespace state established after a mutating filesystem failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalMutationState {
    /// No destination publication began.
    NotPublished,
    /// Destination publication completed before a later failure.
    Published,
    /// A retained temporary entry still requires explicit cleanup.
    CleanupRequired,
    /// Native evidence cannot determine the final namespace state safely.
    Indeterminate,
}
