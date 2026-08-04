// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by walker integration tests.

use super::LocalWalkErrorPolicy;
use super::{
    LocalDirectoryReopenPolicy,
    LocalSymlinkPolicy,
};

/// Options fixed for the lifetime of a local directory walker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "list options have no effect unless they are used"]
pub struct LocalListOptions {
    /// Maximum directory handles retained by a recursive walker.
    max_open_directories: usize,
    /// Policy used when the recursive stack reaches the handle budget.
    reopen_policy: LocalDirectoryReopenPolicy,
    /// Whether child directories should be traversed.
    recursive: bool,
    /// Optional policy overriding the owning filesystem's default.
    symlink_policy: Option<LocalSymlinkPolicy>,
    /// Maximum yielded descendant depth, where immediate children have depth
    /// one.
    max_depth: Option<usize>,
    /// Policy applied when an entry or descendant cannot be inspected.
    error_policy: LocalWalkErrorPolicy,
}

impl LocalListOptions {
    /// Creates a non-recursive listing policy that inherits the filesystem's
    /// symbolic-link policy.
    #[inline]
    pub const fn new() -> Self {
        Self {
            max_open_directories: 64,
            reopen_policy: LocalDirectoryReopenPolicy::Reopen,
            recursive: false,
            symlink_policy: None,
            max_depth: None,
            error_policy: LocalWalkErrorPolicy::FailFast,
        }
    }

    /// Reports whether child directories are traversed.
    #[must_use]
    #[inline(always)]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Returns the optional policy override.
    #[must_use]
    #[inline(always)]
    pub const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }

    /// Returns the maximum yielded depth, or `None` for no explicit limit.
    #[must_use]
    #[inline(always)]
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    /// Returns the maximum number of concurrently open directory handles.
    #[must_use]
    #[inline(always)]
    pub const fn max_open_directories(&self) -> usize {
        self.max_open_directories
    }

    /// Returns the policy used after the handle budget is reached.
    #[inline(always)]
    pub const fn reopen_policy(&self) -> LocalDirectoryReopenPolicy {
        self.reopen_policy
    }

    /// Returns the policy applied after an iteration error.
    #[inline(always)]
    pub const fn error_policy(&self) -> LocalWalkErrorPolicy {
        self.error_policy
    }

    /// Enables recursive traversal.
    #[inline(always)]
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Overrides the owning filesystem's symbolic-link policy.
    #[inline(always)]
    pub const fn with_symlink_policy(
        mut self,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Self {
        self.symlink_policy = Some(symlink_policy);
        self
    }

    /// Limits yielded entries to the specified descendant depth.
    ///
    /// # Parameters
    ///
    /// - `max_depth`: Maximum depth; zero yields no entries.
    #[inline(always)]
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = Some(max_depth);
        self
    }

    /// Sets the maximum number of concurrently open directory handles.
    ///
    /// A value of zero is invalid and is rejected when a walker is opened.
    #[inline(always)]
    pub const fn with_max_open_directories(
        mut self,
        max_open_directories: usize,
    ) -> Self {
        self.max_open_directories = max_open_directories;
        self
    }

    /// Sets the policy used after the handle budget is reached.
    #[inline(always)]
    pub const fn with_reopen_policy(
        mut self,
        reopen_policy: LocalDirectoryReopenPolicy,
    ) -> Self {
        self.reopen_policy = reopen_policy;
        self
    }

    /// Sets the policy applied after an iteration error.
    #[inline(always)]
    pub const fn with_error_policy(
        mut self,
        error_policy: LocalWalkErrorPolicy,
    ) -> Self {
        self.error_policy = error_policy;
        self
    }
}

impl Default for LocalListOptions {
    /// Returns the adaptive listing policy with reader reopening enabled.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
