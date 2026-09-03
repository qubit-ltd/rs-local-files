// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by walker integration tests.

use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileMetadata;

/// One entry yielded by a native local directory walker.
#[derive(Clone, Debug)]
#[must_use]
pub struct LocalDirectoryEntry {
    /// Reusable namespace-absolute identity path.
    path: PathBuf,
    /// Non-authoritative path captured for diagnostics.
    diagnostic_path: Option<PathBuf>,
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
    /// - `path`: Namespace-absolute path reusable with the owning filesystem.
    /// - `diagnostic_path`: Optional native path captured for diagnostics.
    ///   Rooted traversal keeps descriptor authority even after this path
    ///   changes.
    /// - `relative_path`: Path relative to the listing root.
    /// - `metadata`: Normalized entry metadata.
    pub(crate) const fn new(
        path: PathBuf,
        relative_path: PathBuf,
        diagnostic_path: Option<PathBuf>,
        metadata: LocalFileMetadata,
    ) -> Self {
        Self {
            path,
            diagnostic_path,
            relative_path,
            metadata,
        }
    }

    /// Returns the reusable namespace-absolute identity path.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the non-authoritative native path captured for diagnostics.
    ///
    /// Rooted walkers retain descriptor authority, so callers must use
    /// [`Self::path`] for a reusable namespace identity.
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.diagnostic_path.as_deref()
    }

    /// Returns the path relative to the listing root.
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns normalized metadata observed during traversal.
    #[must_use = "inspect the observed metadata"]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }
}
