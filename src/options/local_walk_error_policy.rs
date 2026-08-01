// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by walker integration tests.

/// Controls whether a directory walker stops after an iteration error.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub enum LocalWalkErrorPolicy {
    /// Stop the walker after returning the first error.
    #[default]
    FailFast,
    /// Return errors while allowing later entries to be observed.
    Continue,
}
