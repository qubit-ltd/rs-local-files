// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Facade for the private recursive directory-copy pipeline.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::path::Path;

use super::copy_dir_result::CopyDirResult;
use super::error::with_copy_context;
use super::traversal::copy_dir_iterative;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::local::internal::path_operations::absolute_path;
use crate::local::internal::path_operations::canonicalize_existing_prefix;

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
    copy_dir_all_with_scope(src, dst, options, None)
}

/// Recursively copies a directory tree while constraining followed directory
/// links to a canonical scope root.
pub(crate) fn copy_dir_all_with_paths_scoped(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    scope_root: &Path,
) -> CopyDirResult<LocalCopyDirStats> {
    copy_dir_all_with_scope(src, dst, options, Some(scope_root))
}

/// Runs the recursive copy pipeline after resolving source, destination, and
/// optional scope paths.
///
/// # Parameters
///
/// * `src` - Source directory path.
/// * `dst` - Destination directory path.
/// * `options` - Recursive-copy policies.
/// * `scope_root` - Optional canonical root constraining followed links.
///
/// # Returns
/// Exact statistics accumulated by the copy pipeline.
///
/// # Errors
///
/// Returns a structured copy error when path resolution, scope
/// canonicalization, traversal, staging, publication, or statistics accounting
/// fails.
fn copy_dir_all_with_scope(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    scope_root: Option<&Path>,
) -> CopyDirResult<LocalCopyDirStats> {
    let mut stats = LocalCopyDirStats::default();
    let source_result = absolute_path(src);
    #[cfg(feature = "test-support")]
    let source_result = if crate::local::internal::test_support::is_enabled("copy-source-absolute") {
        Err(crate::local::test_fault_error())
    } else {
        source_result
    };
    let src = with_copy_context(source_result, LocalCopyDirStage::InspectSource, src, dst, &stats)?;
    let destination_result = if dst.as_os_str().is_empty() {
        Ok(dst.to_path_buf())
    } else {
        absolute_path(dst)
    };
    #[cfg(feature = "test-support")]
    let destination_result = if crate::local::internal::test_support::is_enabled("copy-destination-absolute") {
        Err(crate::local::test_fault_error())
    } else {
        destination_result
    };
    let dst = with_copy_context(
        destination_result,
        LocalCopyDirStage::PrepareDestination,
        &src,
        dst,
        &stats,
    )?;
    let destination_root = with_copy_context(
        canonicalize_existing_prefix(&dst),
        LocalCopyDirStage::PrepareDestination,
        &src,
        &dst,
        &stats,
    )?;
    let scope_root = scope_root
        .map(std::fs::canonicalize)
        .transpose()
        .map_err(|error| super::error::copy_dir_error(LocalCopyDirStage::InspectSource, &src, &dst, &stats, error))?;
    copy_dir_iterative(
        &src,
        &dst,
        options,
        &destination_root,
        scope_root.as_deref(),
        &mut stats,
    )?;
    Ok(stats)
}
