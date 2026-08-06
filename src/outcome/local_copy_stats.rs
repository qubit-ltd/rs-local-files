// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.

use crate::local::LocalCopyDirStats;

/// Statistics collected by a unified native copy operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct LocalCopyStats {
    /// Number of regular files copied.
    files: u64,
    /// Number of destination directories created.
    directories: u64,
    /// Number of regular-file bytes copied.
    bytes: u64,
    /// Number of existing destinations skipped.
    skipped: u64,
    /// Number of existing entries replaced or merged.
    overwritten: u64,
}

impl LocalCopyStats {
    /// Creates statistics for one destination entry skipped before transfer.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub(crate) const fn skipped_one() -> Self {
        Self {
            files: 0,
            directories: 0,
            bytes: 0,
            skipped: 1,
            overwritten: 0,
        }
    }

    /// Converts statistics from the shared native copy implementation.
    ///
    /// # Parameters
    ///
    /// - `stats`: Internal copy statistics.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
    pub(crate) const fn from_internal(stats: LocalCopyDirStats) -> Self {
        Self {
            files: stats.files,
            directories: stats.directories,
            bytes: stats.bytes,
            skipped: stats.skipped,
            overwritten: stats.overwritten,
        }
    }

    /// Returns the number of regular files copied.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn files(self) -> u64 {
        self.files
    }

    /// Returns the number of destination directories created.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn directories(self) -> u64 {
        self.directories
    }

    /// Returns the number of regular-file bytes copied.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    /// Returns the number of existing file destinations skipped.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn skipped(self) -> u64 {
        self.skipped
    }

    /// Returns the number of destinations replaced or merged.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn overwritten(self) -> u64 {
        self.overwritten
    }
}
