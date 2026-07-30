// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered through operation option integration tests.

/// Required storage durability for a completed operation.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalDurabilityRequirement {
    /// File data and relevant parent namespace updates must be synchronized.
    Required,
    /// Prefer synchronization but permit an explicitly reported downgrade.
    Preferred,
    /// Do not require explicit synchronization.
    #[default]
    NotRequired,
}
