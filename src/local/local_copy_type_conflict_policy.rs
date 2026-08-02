// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Copy type-conflict policy.
// qubit-style: allow source-test-pair

/// Conflict policy when source and destination entry types differ.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalCopyTypeConflictPolicy {
    /// Fail without removing the destination entry.
    #[default]
    Fail,
    /// Remove the destination entry, including a directory tree, and replace
    /// it with the source entry.
    Replace,
    /// Keep the existing destination entry and skip the conflicting source
    /// entry.
    Skip,
}
