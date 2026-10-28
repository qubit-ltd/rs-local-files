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
    max_ancestors: Option<usize>,
    max_path_bytes: Option<usize>,
    deadline: Option<Duration>,
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

    pub(crate) const fn max_ancestors(&self) -> Option<usize> {
        self.max_ancestors
    }

    pub(crate) const fn max_path_bytes(&self) -> Option<usize> {
        self.max_path_bytes
    }

    pub(crate) const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }
    pub(crate) const fn resolution_limits(&self) -> Option<LocalResolutionLimits> {
        self.resolution_limits
    }
}
