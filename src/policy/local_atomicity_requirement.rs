// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered through operation policy integration tests.

/// Required atomicity for a namespace publication operation.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalAtomicityRequirement {
    /// Success must be atomic and unsupported guarantees fail before side
    /// effects.
    Required,
    /// Prefer an atomic method but permit a reported non-atomic result.
    #[default]
    Preferred,
    /// Do not require atomicity, although an implementation may still provide
    /// it.
    NotRequired,
}
