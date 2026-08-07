// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive directory copy statistics.
// qubit-style: allow source-test-pair

/// Statistics reported by recursive directory copy operations.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) struct LocalCopyDirStats {
    /// Number of regular files copied.
    pub files: u64,

    /// Number of destination directories created.
    pub directories: u64,

    /// Number of bytes copied from regular files.
    pub bytes: u64,

    /// Number of existing destination file entries skipped.
    pub skipped: u64,

    /// Number of existing destination entries replaced or merged.
    pub overwritten: u64,
    /// Whether a completed file publication required a prior directory
    /// removal.
    pub(crate) non_atomic_publication: bool,
    /// Whether every copied regular file was synchronized before publication.
    pub(crate) files_durable: bool,
}

impl Default for LocalCopyDirStats {
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
    /// Returns the number of regular files copied.
    ///
    /// # Returns
    /// Copied regular-file count.
    #[must_use]
    pub(crate) const fn files(&self) -> u64 {
        self.files
    }

    /// Returns the number of destination directories created.
    ///
    /// # Returns
    /// Created directory count.
    #[must_use]
    pub(crate) const fn directories(&self) -> u64 {
        self.directories
    }

    /// Returns the number of bytes copied from regular files.
    ///
    /// # Returns
    /// Copied byte count.
    #[must_use]
    pub(crate) const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Returns the number of existing destination entries skipped.
    ///
    /// # Returns
    /// Skipped entry count.
    #[must_use]
    pub(crate) const fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Returns the number of destination entries overwritten.
    #[must_use]
    pub(crate) const fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// Reports whether every completed file publication was atomic.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn atomic_publication(&self) -> bool {
        !self.non_atomic_publication
    }

    /// Reports whether every copied file was synchronized before publication.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn files_durable(&self) -> bool {
        self.files_durable
    }
}
