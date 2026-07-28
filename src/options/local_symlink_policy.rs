// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy and walker integration tests.

/// Policy for symbolic links encountered during traversal.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LocalSymlinkPolicy {
    /// Reject symbolic links.
    #[default]
    Reject,
    /// Follow links while applying cycle and containment checks.
    Follow,
    /// Recreate links instead of copying their referents.
    Preserve,
}
