// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.

/// Policy for crossing native filesystem device or volume boundaries.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LocalCrossDevicePolicy {
    /// Permit traversal or byte transfer across a device boundary.
    #[default]
    Allow,
    /// Reject a detected device or volume boundary.
    Reject,
}
