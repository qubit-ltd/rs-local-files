// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by walker integration tests.

use std::path::{
    Path,
    PathBuf,
};

use crate::LocalFileMetadata;

/// One entry yielded by a native local directory walker.
#[derive(Clone, Debug)]
#[must_use]
pub struct LocalDirectoryEntry {
    /// Bound host path.
    path: PathBuf,
    /// Path relative to the walker root.
    relative_path: PathBuf,
    /// Metadata observed using the walker's symlink policy.
    metadata: LocalFileMetadata,
}

impl LocalDirectoryEntry {
    /// Creates a yielded directory entry.
    ///
    /// # Parameters
    ///
    /// - `path`: Bound native host path.
    /// - `relative_path`: Path relative to the listing root.
    /// - `metadata`: Normalized entry metadata.
    #[inline(always)]
    pub(crate) const fn new(
        path: PathBuf,
        relative_path: PathBuf,
        metadata: LocalFileMetadata,
    ) -> Self {
        Self {
            path,
            relative_path,
            metadata,
        }
    }

    /// Returns the bound native host path.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the path relative to the listing root.
    #[must_use]
    #[inline(always)]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns normalized metadata observed during traversal.
    #[inline(always)]
    pub const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }
}
