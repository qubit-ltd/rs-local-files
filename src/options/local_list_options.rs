// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by walker integration tests.

/// Options fixed for the lifetime of a local directory walker.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalListOptions {
    /// Whether child directories should be traversed.
    recursive: bool,
    /// Whether symbolic links to directories should be traversed.
    follow_symlinks: bool,
    /// Maximum yielded descendant depth, where immediate children have depth
    /// one.
    max_depth: Option<usize>,
}

impl LocalListOptions {
    /// Creates a non-recursive, no-follow listing policy.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            recursive: false,
            follow_symlinks: false,
            max_depth: None,
        }
    }

    /// Reports whether child directories are traversed.
    #[must_use]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Reports whether symbolic links are followed.
    #[must_use]
    pub const fn follows_symlinks(&self) -> bool {
        self.follow_symlinks
    }

    /// Returns the maximum yielded depth, or `None` for no explicit limit.
    #[must_use]
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    /// Enables recursive traversal.
    #[must_use]
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Enables symbolic-link following with cycle detection.
    #[must_use]
    pub const fn with_follow_symlinks(mut self) -> Self {
        self.follow_symlinks = true;
        self
    }

    /// Limits yielded entries to the specified descendant depth.
    ///
    /// # Parameters
    ///
    /// - `max_depth`: Maximum depth; zero yields no entries.
    #[must_use]
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = Some(max_depth);
        self
    }
}
