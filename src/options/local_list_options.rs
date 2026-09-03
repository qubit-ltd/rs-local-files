// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by walker integration tests.

use std::time::Duration;

use super::LocalDirectoryReopenPolicy;
use super::LocalWalkErrorPolicy;
use crate::policy::LocalSymlinkPolicy;

/// Options fixed for the lifetime of a local directory walker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "list options have no effect unless they are used"]
pub struct LocalListOptions {
    /// Maximum directory handles retained by a recursive walker.
    max_open_directories: Option<usize>,
    /// Policy used when the recursive stack reaches the handle budget.
    reopen_policy: LocalDirectoryReopenPolicy,
    /// Whether child directories should be traversed.
    recursive: bool,
    /// Optional policy overriding the owning filesystem's default.
    symlink_policy: Option<LocalSymlinkPolicy>,
    /// Maximum yielded descendant depth, where immediate children have depth
    /// one.
    max_depth: Option<usize>,
    /// Optional cap on entries yielded by this walker.
    max_entries: Option<usize>,
    /// Optional cap on cumulative names observed by duplicate-name tracking.
    max_seen_name_bytes: Option<usize>,
    /// Optional elapsed-time budget for the complete traversal.
    deadline: Option<Duration>,
    /// Policy applied when an entry or descendant cannot be inspected.
    error_policy: LocalWalkErrorPolicy,
}

impl LocalListOptions {
    /// Creates a non-recursive listing policy that inherits the filesystem's
    /// symbolic-link policy.
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn new() -> Self {
        Self {
            max_open_directories: None,
            reopen_policy: LocalDirectoryReopenPolicy::Reopen,
            recursive: false,
            symlink_policy: None,
            max_depth: None,
            max_entries: None,
            max_seen_name_bytes: None,
            deadline: None,
            error_policy: LocalWalkErrorPolicy::FailFast,
        }
    }

    /// Reports whether child directories are traversed.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Returns the optional policy override.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }

    /// Returns the maximum yielded depth, or `None` for no explicit limit.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    /// Returns the maximum number of entries yielded by this walker.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn max_entries(&self) -> Option<usize> {
        self.max_entries
    }

    /// Returns the maximum cumulative name bytes observed by duplicate-name
    /// tracking.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn max_seen_name_bytes(&self) -> Option<usize> {
        self.max_seen_name_bytes
    }

    /// Returns the optional elapsed-time budget.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }

    /// Returns the maximum number of concurrently open directory handles.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn max_open_directories(&self) -> Option<usize> {
        self.max_open_directories
    }

    /// Returns the policy used after the handle budget is reached.
    #[must_use = "inspect the directory reopen policy"]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn reopen_policy(&self) -> LocalDirectoryReopenPolicy {
        self.reopen_policy
    }

    /// Returns the policy applied after an iteration error.
    #[must_use = "inspect the listing error policy"]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn error_policy(&self) -> LocalWalkErrorPolicy {
        self.error_policy
    }

    /// Enables recursive traversal.
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Overrides the owning filesystem's symbolic-link policy.
    pub const fn with_symlink_policy(mut self, symlink_policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(symlink_policy);
        self
    }

    /// Limits yielded entries to the specified descendant depth.
    ///
    /// # Parameters
    ///
    /// - `max_depth`: Maximum depth; zero yields no entries.
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = Some(max_depth);
        self
    }

    /// Removes the yielded-depth budget.
    pub const fn without_max_depth(mut self) -> Self {
        self.max_depth = None;
        self
    }

    /// Limits the number of entries yielded by the walker.
    pub const fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = Some(max_entries);
        self
    }

    /// Removes the yielded-entry budget.
    pub const fn without_max_entries(mut self) -> Self {
        self.max_entries = None;
        self
    }

    /// Limits cumulative name bytes observed by duplicate-name tracking.
    ///
    /// Capacity is not released when a completed directory frame is popped.
    pub const fn with_max_seen_name_bytes(mut self, max_seen_name_bytes: usize) -> Self {
        self.max_seen_name_bytes = Some(max_seen_name_bytes);
        self
    }

    /// Removes the duplicate-name memory budget.
    pub const fn without_max_seen_name_bytes(mut self) -> Self {
        self.max_seen_name_bytes = None;
        self
    }

    /// Sets the maximum elapsed time for the complete traversal.
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Removes the traversal deadline.
    pub const fn without_deadline(mut self) -> Self {
        self.deadline = None;
        self
    }

    /// Sets the maximum number of concurrently open directory handles.
    ///
    /// A value of zero is invalid and is rejected when a walker is opened.
    pub const fn with_max_open_directories(mut self, max_open_directories: usize) -> Self {
        self.max_open_directories = Some(max_open_directories);
        self
    }

    /// Removes the concurrently-open-directory budget.
    pub const fn without_max_open_directories(mut self) -> Self {
        self.max_open_directories = None;
        self
    }

    /// Sets the policy used after the handle budget is reached.
    pub const fn with_reopen_policy(mut self, reopen_policy: LocalDirectoryReopenPolicy) -> Self {
        self.reopen_policy = reopen_policy;
        self
    }

    /// Sets the policy applied after an iteration error.
    pub const fn with_error_policy(mut self, error_policy: LocalWalkErrorPolicy) -> Self {
        self.error_policy = error_policy;
        self
    }
}

impl Default for LocalListOptions {
    /// Returns the adaptive listing policy with reader reopening enabled.
    fn default() -> Self {
        Self::new()
    }
}
