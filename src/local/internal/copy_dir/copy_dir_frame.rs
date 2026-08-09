// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! One active directory frame for iterative recursive-copy traversal.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use super::super::directory_identity::DirectoryIdentity;

/// Holds the lazy iterator and completion state for one source directory.
#[must_use = "discarding an active copy frame abandons its directory traversal"]
pub(super) struct CopyDirFrame {
    /// Source directory path used for traversal and diagnostics.
    src: PathBuf,
    /// Destination directory path paired with `src`.
    dst: PathBuf,
    /// Filesystem-object identity retained for active-cycle detection.
    source_identity: DirectoryIdentity,
    /// Source permissions captured for post-order preservation.
    source_permissions: fs::Permissions,
    /// Lazy iterator over direct source-directory entries.
    entries: fs::ReadDir,
}

impl CopyDirFrame {
    /// Creates one active directory traversal frame.
    ///
    /// # Parameters
    ///
    /// * `src` - Source directory path.
    /// * `dst` - Destination directory path.
    /// * `source_identity` - Filesystem-object identity for cycle detection.
    /// * `source_permissions` - Permissions to apply after copying children.
    /// * `entries` - Lazy source-directory iterator.
    ///
    /// # Returns
    ///
    /// A frame ready to yield source entries.
    pub(super) fn new(
        src: PathBuf,
        dst: PathBuf,
        source_identity: DirectoryIdentity,
        source_permissions: fs::Permissions,
        entries: fs::ReadDir,
    ) -> Self {
        Self {
            src,
            dst,
            source_identity,
            source_permissions,
            entries,
        }
    }

    /// Returns the source directory path.
    #[must_use]
    pub(super) fn src(&self) -> &Path {
        &self.src
    }

    /// Returns the destination directory path.
    #[must_use]
    pub(super) fn dst(&self) -> &Path {
        &self.dst
    }

    /// Returns the filesystem-object source identity.
    #[must_use]
    pub(super) const fn source_identity(&self) -> &DirectoryIdentity {
        &self.source_identity
    }

    /// Returns the source permissions captured before traversal.
    #[must_use]
    pub(super) fn source_permissions(&self) -> &fs::Permissions {
        &self.source_permissions
    }

    /// Advances the lazy source-directory iterator.
    ///
    /// # Returns
    ///
    /// The next directory-entry result, or `None` after the directory is
    /// exhausted.
    ///
    /// # Errors
    ///
    /// The yielded result contains the filesystem error when an entry cannot
    /// be read.
    pub(super) fn next_entry(
        &mut self,
    ) -> Option<std::io::Result<fs::DirEntry>> {
        self.entries.next()
    }
}
