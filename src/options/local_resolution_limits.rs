// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Additional per-resolution budgets. `None` means no caller limit; zero is a
/// real limit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct LocalResolutionLimits {
    symlink_hops: Option<usize>,
    symlink_target_bytes: Option<usize>,
    actual_components: Option<usize>,
    resolved_path_bytes: Option<usize>,
}
impl LocalResolutionLimits {
    /// Creates limits with no caller-added caps.
    pub const fn new() -> Self {
        Self {
            symlink_hops: None,
            symlink_target_bytes: None,
            actual_components: None,
            resolved_path_bytes: None,
        }
    }
    /// Caps symlink hops; zero forbids following any symlink.
    pub const fn with_max_symlink_hops(mut self, n: usize) -> Self {
        self.symlink_hops = Some(n);
        self
    }
    /// Caps native encoded bytes in each symlink target; zero permits no target
    /// bytes.
    pub const fn with_max_symlink_target_bytes(mut self, n: usize) -> Self {
        self.symlink_target_bytes = Some(n);
        self
    }
    /// Caps pending and resolved path components; zero permits no normal
    /// components.
    pub const fn with_max_actual_components(mut self, n: usize) -> Self {
        self.actual_components = Some(n);
        self
    }
    /// Caps pending and resolved native path bytes; zero permits no path bytes.
    pub const fn with_max_resolved_path_bytes(mut self, n: usize) -> Self {
        self.resolved_path_bytes = Some(n);
        self
    }

    /// Intersects this budget with a child budget, treating `None` as
    /// unlimited.
    pub(crate) const fn tightened(self, child: Self) -> Self {
        Self {
            symlink_hops: min_limit(self.symlink_hops, child.symlink_hops),
            symlink_target_bytes: min_limit(self.symlink_target_bytes, child.symlink_target_bytes),
            actual_components: min_limit(self.actual_components, child.actual_components),
            resolved_path_bytes: min_limit(self.resolved_path_bytes, child.resolved_path_bytes),
        }
    }
    pub(crate) const fn symlink_hops(&self) -> Option<usize> {
        self.symlink_hops
    }
    pub(crate) const fn symlink_target_bytes(&self) -> Option<usize> {
        self.symlink_target_bytes
    }
    pub(crate) const fn actual_components(&self) -> Option<usize> {
        self.actual_components
    }
    pub(crate) const fn resolved_path_bytes(&self) -> Option<usize> {
        self.resolved_path_bytes
    }
}

const fn min_limit(parent: Option<usize>, child: Option<usize>) -> Option<usize> {
    match (parent, child) {
        (Some(parent), Some(child)) => Some(if parent < child { parent } else { child }),
        (Some(parent), None) => Some(parent),
        (None, Some(child)) => Some(child),
        (None, None) => None,
    }
}
