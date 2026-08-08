// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup state reported by temporary-resource persistence.
// qubit-style: allow source-test-pair

/// Cleanup state achieved after a temporary resource was published.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPersistCleanupState {
    /// The private temporary sandbox was removed successfully.
    Complete,
    /// Publication succeeded, but the private temporary sandbox remains.
    ResidualSandbox,
}
