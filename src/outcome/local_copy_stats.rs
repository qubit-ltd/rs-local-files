// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.
// qubit-style: allow inline-tests
// qubit-style: allow explicit-imports

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
    pub const fn files(self) -> u64 {
        self.files
    }

    /// Returns the number of destination directories created.
    #[must_use]
    pub const fn directories(self) -> u64 {
        self.directories
    }

    /// Returns the number of regular-file bytes copied.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    /// Returns the number of existing file destinations skipped.
    #[must_use]
    pub const fn skipped(self) -> u64 {
        self.skipped
    }

    /// Returns the number of destinations replaced or merged.
    #[must_use]
    pub const fn overwritten(self) -> u64 {
        self.overwritten
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::LocalCopyDirStats;

    #[test]
    fn exposes_skipped_and_internal_counts() {
        let skipped = LocalCopyStats::skipped_one();
        assert_eq!(skipped.skipped(), 1);
        let stats = LocalCopyStats::from_internal(LocalCopyDirStats {
            files: 1,
            directories: 2,
            bytes: 3,
            skipped: 4,
            overwritten: 5,
            non_atomic_publication: false,
            files_durable: true,
        });
        assert_eq!(
            (stats.files(), stats.directories(), stats.bytes()),
            (1, 2, 3)
        );
        assert_eq!((stats.skipped(), stats.overwritten()), (4, 5));
    }
}
