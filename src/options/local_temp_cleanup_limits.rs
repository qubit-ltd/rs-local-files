// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resource limits retained by a temporary directory for explicit and Drop
//! cleanup.
// qubit-style: allow source-test-pair
// Covered through public temporary cleanup integration tests.

use std::time::Duration;

use super::LocalDeleteOptions;

/// Optional limits for each temporary-directory cleanup attempt.
///
/// The root has depth zero and counts as one entry. Pending-path bytes count
/// native encoded paths queued by the shared scheduler, excluding allocator
/// overhead, readers and the currently enumerated entry. Traversal is lazy but
/// retains queued paths; it does not promise constant memory or deletion order.
/// Each cleanup, including Drop, starts a fresh budget with these same limits.
/// The private sandbox costs at most one additional removal outside the entry
/// and path budgets, and shares the cleanup attempt's cooperative deadline.
/// Native calls cannot be interrupted by this deadline. Defaults are unbounded.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "cleanup limits have no effect unless retained by a temporary directory"]
pub struct LocalTempCleanupLimits {
    /// Maximum descendant depth below the root at zero.
    max_depth: Option<usize>,
    /// Maximum discovered source entries, including the root.
    max_entries: Option<usize>,
    /// Maximum native encoded bytes in queued paths.
    max_pending_path_bytes: Option<usize>,
    /// Cooperative elapsed-time limit for one complete cleanup attempt.
    deadline: Option<Duration>,
}

impl LocalTempCleanupLimits {
    /// Creates unbounded limits; no validation or I/O is performed.
    pub const fn new() -> Self {
        Self {
            max_depth: None,
            max_entries: None,
            max_pending_path_bytes: None,
            deadline: None,
        }
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

    /// Returns the optional cooperative elapsed-time limit for recursive
    /// deletion.
    #[must_use]
    pub const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }

    /// Sets the cooperative elapsed-time limit for recursive deletion.
    pub const fn with_deadline(mut self, limit: Duration) -> Self {
        self.deadline = Some(limit);
        self
    }

    /// Removes the cooperative elapsed-time limit for recursive deletion.
    pub const fn without_deadline(mut self) -> Self {
        self.deadline = None;
        self
    }

    /// Converts retained limits to strict recursive deletion without changing
    /// missing-entry or source ownership semantics; performs no I/O.
    pub(crate) const fn delete_options(self) -> LocalDeleteOptions {
        let mut options = LocalDeleteOptions::new().with_recursive();
        if let Some(limit) = self.max_depth {
            options = options.with_max_depth(limit);
        }
        if let Some(limit) = self.max_entries {
            options = options.with_max_entries(limit);
        }
        if let Some(limit) = self.max_pending_path_bytes {
            options = options.with_max_pending_path_bytes(limit);
        }
        if let Some(limit) = self.deadline {
            options = options.with_deadline(limit);
        }
        options
    }
}
