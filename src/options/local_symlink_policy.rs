// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy and walker integration tests.

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
    /// Follow links without applying a Rooted root boundary. This is an
    /// explicit opt-in for rooted operations that intentionally address the
    /// object reached outside the opened root.
    FollowAcrossScope,
    /// Legacy alias for normal follow behavior. It resolves to
    /// `FollowWithinScope` for Rooted and `FollowAcrossScope` for Host.
    Follow,
}

impl LocalSymlinkPolicy {
    /// Reports whether path resolution may follow a symbolic link.
    #[must_use]
    #[inline(always)]
    pub const fn follows(self) -> bool {
        !matches!(self, Self::Reject)
    }

    /// Resolves the legacy follow value for a concrete filesystem scope.
    #[must_use = "the normalized scope policy must be used"]
    #[inline(always)]
    pub const fn for_scope(self, rooted: bool) -> Self {
        match self {
            Self::Reject => Self::Reject,
            Self::FollowWithinScope => Self::FollowWithinScope,
            Self::FollowAcrossScope => Self::FollowAcrossScope,
            Self::Follow => {
                if rooted {
                    Self::FollowWithinScope
                } else {
                    Self::FollowAcrossScope
                }
            }
        }
    }
}
