// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Namespace certainty for a temporary resource.
// qubit-style: allow source-test-pair
// Covered through the public temporary-resource integration tests.

/// Namespace certainty retained after a temporary-resource state transition.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalTempResourceState {
    /// The source is known to remain owned and cleanup-safe.
    Owned,
    /// The native namespace result is unknown; no cleanup operation is safe.
    Indeterminate,
    /// The resource was kept, cleaned, or fully persisted.
    Released,
}
