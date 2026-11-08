// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by walker integration tests.

use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileMetadata;

/// One entry yielded by a native local directory walker.
///
/// The walker supplies entries lazily; their paths identify the entry in the
/// filesystem namespace, while diagnostic paths are only observations of the
/// native path at traversal time.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> qubit_local_files::LocalResult<()> {
/// let filesystem = LocalFileSystem::host()?;
/// let walker = filesystem.list(Path::new("."))?;
/// for entry in walker {
///     let entry = entry?;
///     println!("{} ({})", entry.path().display(), entry.metadata().len());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct LocalDirectoryEntry {
    /// Reusable namespace-absolute identity path.
    path: PathBuf,
    /// Non-authoritative path captured for diagnostics.
    diagnostic_path: Option<PathBuf>,
    /// Path relative to the walker root.
    relative_path: PathBuf,
    /// Metadata for the entry itself, without following its final symlink.
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
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the non-authoritative native path captured for diagnostics.
    ///
    /// Rooted walkers retain descriptor authority, so callers must use
    /// [`Self::path`] for a reusable namespace identity.
    ///
    /// # Returns
    ///
    /// `Some(path)` contains the native diagnostic path; `None` means that no
    /// such path was captured. This path is not an authority for filesystem
    /// operations.
    #[must_use]
    #[inline]
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.diagnostic_path.as_deref()
    }

    /// Returns the path relative to the listing root.
    #[must_use]
    #[inline]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns normalized metadata observed during traversal.
    #[must_use = "inspect the observed metadata"]
    #[inline]
    pub const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }
}
