// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive directory copy statistics.
// qubit-style: allow source-test-pair
// qubit-style: allow explicit-imports

/// Statistics reported by recursive directory copy operations.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalCopyDirStats {
    /// Number of regular files and symbolic-link entries copied.
    pub files: u64,

    /// Number of destination directories created.
    pub directories: u64,

    /// Number of bytes copied from regular files.
    pub bytes: u64,

    /// Number of source entries skipped because of destination conflicts.
    pub skipped: u64,

    /// Number of existing destination entries replaced, including directories
    /// merged under the Overwrite conflict policy.
    pub overwritten: u64,
    /// Whether a completed publication used direct symbolic-link creation or
    /// required a prior directory removal.
    pub non_atomic_publication: bool,
    /// Whether all copied regular files were synchronized, with no copied
    /// symbolic-link entry. Parent-directory synchronization is tracked
    /// separately.
    pub files_durable: bool,
}

impl Default for LocalCopyDirStats {
    /// Creates empty progress with durability preserved until disproven.
    fn default() -> Self {
        Self {
            files: 0,
            directories: 0,
            bytes: 0,
            skipped: 0,
            overwritten: 0,
            non_atomic_publication: false,
            files_durable: true,
        }
    }
}
#[allow(dead_code)]
impl LocalCopyDirStats {
    /// Returns the number of regular files and symbolic-link entries copied.
    ///
    /// # Returns
    /// Copied non-directory entry count.
    #[must_use]
    pub const fn files(&self) -> u64 {
        self.files
    }

    /// Returns the number of destination directories created.
    ///
    /// # Returns
    /// Created directory count.
    #[must_use]
    pub const fn directories(&self) -> u64 {
        self.directories
    }

    /// Returns the number of bytes copied from regular files.
    ///
    /// # Returns
    /// Copied byte count.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Returns the number of existing destination entries skipped.
    ///
    /// # Returns
    /// Skipped entry count.
    #[must_use]
    pub const fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Returns the number of destination entries overwritten.
    #[must_use]
    pub const fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// Reports whether every completed file publication was atomic.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn atomic_publication(&self) -> bool {
        !self.non_atomic_publication
    }

    /// Reports whether every copied file was synchronized before publication.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn files_durable(&self) -> bool {
        self.files_durable
    }
}
