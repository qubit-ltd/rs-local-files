// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Facade for the private recursive directory-copy pipeline.

use std::path::Path;

use crate::{
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
};

use crate::local::internal::path_operations::canonicalize_existing_prefix;

use super::error::{
    CopyDirResult,
    with_copy_context,
};
use super::traversal::copy_dir_recursive;

/// Recursively copies a directory tree with the supplied options.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Copy behavior options.
///
/// # Returns
///
/// Exact statistics for copied files, created directories, bytes, and skips.
///
/// # Errors
///
/// Returns a structured error when validation, containment, traversal,
/// staging, commit, permission preservation, or statistics accounting fails.
pub(crate) fn copy_dir_all_with_paths(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
) -> CopyDirResult<LocalCopyDirStats> {
    let mut active_sources = Vec::new();
    let mut stats = LocalCopyDirStats::default();
    let destination_root = with_copy_context(
        canonicalize_existing_prefix(dst),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        &stats,
    )?;
    copy_dir_recursive(
        src,
        dst,
        options,
        &destination_root,
        &mut active_sources,
        &mut stats,
    )?;
    Ok(stats)
}
