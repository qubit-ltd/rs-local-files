// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Selects how a parent component behaves at the resolver's root boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootParentBehavior {
    /// Reject traversal above the virtual root.
    RejectEscape,
    /// Keep traversal at the host root when a parent reaches the virtual root.
    StayAtHostRoot,
}
