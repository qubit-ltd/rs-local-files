// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! One active directory frame for iterative size accumulation.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs;
use std::path::PathBuf;

use super::directory_identity::DirectoryIdentity;

/// Holds the lazy iterator and subtotal for one directory being measured.
#[must_use = "discarding an active size frame abandons its directory subtotal"]
pub(super) struct DirSizeFrame {
    /// Directory path used for traversal and overflow diagnostics.
    path: PathBuf,
    /// Filesystem-object identity retained for active-cycle detection.
    identity: DirectoryIdentity,
    /// Subtotal accumulated from completed children and regular files.
    size: u64,
    /// Lazy iterator over direct directory entries.
    entries: fs::ReadDir,
}

impl DirSizeFrame {
    /// Creates one active directory-size frame.
    ///
    /// # Parameters
    ///
    /// * `path` - The directory path retained for traversal diagnostics.
    /// * `identity` - Filesystem-object identity for cycle detection.
    /// * `entries` - The lazy iterator over the directory's direct entries.
    ///
    /// # Returns
    ///
    /// A frame with a zero subtotal and the supplied lazy iterator.
    #[inline]
    pub(super) fn new(
        path: PathBuf,
        identity: DirectoryIdentity,
        entries: fs::ReadDir,
    ) -> Self {
        Self {
            path,
            identity,
            size: 0,
            entries,
        }
    }

    /// Returns the subtotal accumulated for this directory.
    #[must_use]
    #[inline(always)]
    pub(super) const fn size(&self) -> u64 {
        self.size
    }

    /// Replaces the subtotal accumulated for this directory.
    ///
    /// # Parameters
    ///
    /// * `size` - The replacement subtotal in bytes.
    #[inline(always)]
    pub(super) const fn set_size(&mut self, size: u64) {
        self.size = size;
    }

    /// Advances the lazy directory iterator.
    ///
    /// # Returns
    ///
    /// The next directory-entry result, or `None` after the directory is
    /// exhausted.
    ///
    /// # Errors
    ///
    /// A returned `Some(Err(_))` contains the I/O error reported while reading
    /// the next directory entry.
    #[inline(always)]
    pub(super) fn next_entry(
        &mut self,
    ) -> Option<std::io::Result<fs::DirEntry>> {
        self.entries.next()
    }

    /// Consumes this completed frame into its path, identity, and subtotal.
    ///
    /// # Returns
    ///
    /// The owned directory path, identity, and completed subtotal.
    #[must_use = "the completed directory path, identity, and subtotal must be consumed together"]
    #[inline(always)]
    pub(super) fn into_parts(self) -> (PathBuf, DirectoryIdentity, u64) {
        (self.path, self.identity, self.size)
    }
}
