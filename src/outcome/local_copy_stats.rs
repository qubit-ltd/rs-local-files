// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by copy integration tests.

use crate::local::LocalCopyDirStats;

/// Statistics collected by a unified native copy operation.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
///
/// let filesystem = LocalFileSystem::host()?;
/// let directory = std::env::temp_dir();
/// let source = directory.join(format!("qubit-copy-stats-source-{}", std::process::id()));
/// let target = directory.join(format!("qubit-copy-stats-target-{}", std::process::id()));
/// std::fs::write(&source, b"copy statistics")?;
/// let outcome = filesystem.copy(&source, &target)?;
/// let stats = outcome.stats();
/// assert_eq!(stats.bytes(), 15);
/// assert_eq!(stats.files(), 1);
/// std::fs::remove_file(source)?;
/// std::fs::remove_file(target)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct LocalCopyStats {
    /// Number of regular files and symbolic-link entries copied.
    files: u64,
    /// Number of destination directories created.
    directories: u64,
    /// Number of regular-file bytes copied.
    bytes: u64,
    /// Number of existing destinations skipped.
    skipped: u64,
    /// Number of existing entries replaced, including directories merged under
    /// the Overwrite conflict policy.
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

    /// Returns the number of regular files and symbolic-link entries copied.
    #[must_use]
    #[inline]
    pub const fn files(self) -> u64 {
        self.files
    }

    /// Returns the number of destination directories created.
    #[must_use]
    #[inline]
    pub const fn directories(self) -> u64 {
        self.directories
    }

    /// Returns the number of regular-file bytes copied.
    #[must_use]
    #[inline]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    /// Returns the number of source entries skipped by destination policy.
    #[must_use]
    #[inline]
    pub const fn skipped(self) -> u64 {
        self.skipped
    }

    /// Returns the number of destinations replaced, including directories
    /// merged under the Overwrite conflict policy.
    #[must_use]
    #[inline]
    pub const fn overwritten(self) -> u64 {
        self.overwritten
    }
}
