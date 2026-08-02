// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic-write destination states.
// qubit-style: allow source-test-pair

/// Known state of the destination after an atomic-write failure.
///
/// Additional states may be added if a platform can report a more precise
/// replacement outcome. Downstream matches must retain a wildcard arm.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum LocalAtomicDestinationState {
    /// The destination was not modified by the failed operation.
    ///
    /// The implementation attempts to remove uncommitted staging in this
    /// state; inspect the cleanup error when recovery depends on its removal.
    Unchanged,
    /// The destination contains the staged replacement.
    ///
    /// A later durability or cleanup step failed after installation.
    Replaced,
    /// The destination is known to be absent.
    ///
    /// Any still-existing staging entry is retained for explicit recovery.
    Missing,
    /// The destination outcome cannot be determined reliably.
    ///
    /// Inspect both destination and staging before retrying or deleting either
    /// path.
    Indeterminate,
}
