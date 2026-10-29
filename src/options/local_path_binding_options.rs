// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::time::Duration;

use super::LocalResolutionLimits;

/// Resource budgets for retaining and verifying a source path binding.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "path binding options have no effect unless they are used"]
pub struct LocalPathBindingOptions {
    /// Maximum retained ancestor handles; `None` means unbounded.
    max_ancestors: Option<usize>,
    /// Maximum cumulative native path bytes; `None` means unbounded.
    max_path_bytes: Option<usize>,
    /// Cooperative duration limit; `None` means no configured deadline.
    deadline: Option<Duration>,
    /// Additional resolution constraints; `None` adds no constraints.
    resolution_limits: Option<LocalResolutionLimits>,
}

impl LocalPathBindingOptions {
    /// Creates path-binding options without additional limits.
    pub const fn new() -> Self {
        Self {
            max_ancestors: None,
            max_path_bytes: None,
            deadline: None,
            resolution_limits: None,
        }
    }

    /// Limits retained ancestor directory handles.
    pub const fn with_max_ancestors(mut self, maximum: usize) -> Self {
        self.max_ancestors = Some(maximum);
        self
    }

    /// Limits cumulative native path bytes retained for binding.
    pub const fn with_max_path_bytes(mut self, maximum: usize) -> Self {
        self.max_path_bytes = Some(maximum);
        self
    }

    /// Limits total time spent acquiring and verifying the binding.
    pub const fn with_deadline(mut self, maximum: Duration) -> Self {
        self.deadline = Some(maximum);
        self
    }
    /// Sets limits for resolving and rechecking the bound path.
    ///
    /// These limits are combined with any enclosing filesystem or handle
    /// limits, so they can only make resolution more restrictive.
    pub const fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.resolution_limits = Some(limits);
        self
    }

    /// Returns the optional ancestor-handle limit without performing I/O.
    pub(crate) const fn max_ancestors(&self) -> Option<usize> {
        self.max_ancestors
    }

    /// Returns the optional path-byte limit without performing I/O.
    pub(crate) const fn max_path_bytes(&self) -> Option<usize> {
        self.max_path_bytes
    }

    /// Returns the optional deadline without performing I/O.
    pub(crate) const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }
    /// Returns additional resolution constraints without performing I/O.
    pub(crate) const fn resolution_limits(&self) -> Option<LocalResolutionLimits> {
        self.resolution_limits
    }
}
