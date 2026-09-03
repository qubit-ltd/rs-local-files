// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by authority, copy, and walker integration tests.

/// Policy for symbolic links encountered while resolving a local path.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalSymlinkPolicy {
    /// Reject a path that requires following a symbolic link.
    #[default]
    Reject,
    /// Follow links while requiring a Rooted path to remain within its root.
    /// On Host, this has the same native effect as `FollowAcrossScope` because
    /// Host has no configured root boundary.
    FollowWithinScope,
    /// Follow links without applying a Rooted root boundary.
    ///
    /// This policy is supported only by Host filesystems. Selecting it for a
    /// Rooted filesystem returns `InvalidOptions` because Rooted authority is
    /// limited to its opened root.
    FollowAcrossScope,
}

impl LocalSymlinkPolicy {
    /// Reports whether path resolution may follow a symbolic link.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn follows(self) -> bool {
        !matches!(self, Self::Reject)
    }
}
