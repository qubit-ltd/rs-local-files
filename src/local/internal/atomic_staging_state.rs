// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal staging-name state after an atomic installation failure.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

/// Describes whether an atomic staging name remains safe to clean up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtomicStagingState {
    /// The staging name still refers to the staged file.
    Present,
    /// The staging name may have moved or changed identity.
    Indeterminate,
}
