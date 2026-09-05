// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by delete integration tests.

use std::time::Duration;

/// Options for deleting a native filesystem entry.
///
/// Recursive budgets are opt-in. Entries include the requested root (depth
/// zero). Pending-path bytes count encoded native path lengths retained by the
/// work queue, excluding allocator overhead and in-flight enumeration objects.
/// Deadlines are cooperative checks between native calls, not I/O timeouts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "delete options have no effect unless they are used"]
pub struct LocalDeleteOptions {
    /// Whether directory descendants should be removed.
    recursive: bool,
    /// Whether a missing entry is treated as a successful no-op.
    missing_ok: bool,
    /// Maximum entry depth below the requested directory.
    max_depth: Option<usize>,
    /// Maximum discovered entries, including the requested directory.
    max_entries: Option<usize>,
    /// Maximum encoded path bytes retained for pending deletion work.
    max_pending_path_bytes: Option<usize>,
    /// Maximum elapsed time spent on recursive deletion.
    deadline: Option<Duration>,
}

impl LocalDeleteOptions {
    /// Creates strict, non-recursive deletion options.
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn new() -> Self {
        Self {
            recursive: false,
            missing_ok: false,
            max_depth: None,
            max_entries: None,
            max_pending_path_bytes: None,
            deadline: None,
        }
    }

    /// Reports whether directory deletion is recursive.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Reports whether missing entries are accepted.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn missing_ok(&self) -> bool {
        self.missing_ok
    }

    /// Enables recursive directory deletion.
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Treats a missing entry as a successful no-op.
    pub const fn with_missing_ok(mut self) -> Self {
        self.missing_ok = true;
        self
    }
    /// Returns the optional limit for entry depth, with the requested directory
    /// at depth zero.
    #[must_use]
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    /// Sets the limit for entry depth, with the requested directory at depth
    /// zero.
    pub const fn with_max_depth(mut self, limit: usize) -> Self {
        self.max_depth = Some(limit);
        self
    }

    /// Removes the limit for entry depth, with the requested directory at depth
    /// zero.
    pub const fn without_max_depth(mut self) -> Self {
        self.max_depth = None;
        self
    }

    /// Returns the optional limit for discovered entries, including the
    /// requested directory.
    #[must_use]
    pub const fn max_entries(&self) -> Option<usize> {
        self.max_entries
    }

    /// Sets the limit for discovered entries, including the requested
    /// directory.
    pub const fn with_max_entries(mut self, limit: usize) -> Self {
        self.max_entries = Some(limit);
        self
    }

    /// Removes the limit for discovered entries, including the requested
    /// directory.
    pub const fn without_max_entries(mut self) -> Self {
        self.max_entries = None;
        self
    }

    /// Returns the optional limit for encoded native bytes retained by pending
    /// paths.
    #[must_use]
    pub const fn max_pending_path_bytes(&self) -> Option<usize> {
        self.max_pending_path_bytes
    }

    /// Sets the limit for encoded native bytes retained by pending paths.
    pub const fn with_max_pending_path_bytes(mut self, limit: usize) -> Self {
        self.max_pending_path_bytes = Some(limit);
        self
    }

    /// Removes the limit for encoded native bytes retained by pending paths.
    pub const fn without_max_pending_path_bytes(mut self) -> Self {
        self.max_pending_path_bytes = None;
        self
    }

    /// Returns the optional limit for cooperative elapsed-time limit for
    /// recursive deletion.
    #[must_use]
    pub const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }

    /// Sets the limit for cooperative elapsed-time limit for recursive
    /// deletion.
    pub const fn with_deadline(mut self, limit: Duration) -> Self {
        self.deadline = Some(limit);
        self
    }

    /// Removes the limit for cooperative elapsed-time limit for recursive
    /// deletion.
    pub const fn without_deadline(mut self) -> Self {
        self.deadline = None;
        self
    }
}
