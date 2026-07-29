// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive directory copy statistics.

/// Statistics reported by recursive directory copy operations.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalCopyDirStats {
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
}

impl LocalCopyDirStats {
    /// Returns the number of regular files copied.
    ///
    /// # Returns
    /// Copied regular-file count.
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
}
