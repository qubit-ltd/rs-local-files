// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Facade for the private recursive directory-copy pipeline.
// qubit-style: allow source-test-pair
// qubit-style: allow coverage-cfg
// Private behavior is covered through public integration tests.

#[cfg(coverage)]
use std::io::Error;
use std::path::Path;

use crate::{
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
};

use crate::local::internal::path_operations::{
    absolute_path,
    canonicalize_existing_prefix,
};

use super::copy_dir_result::CopyDirResult;
use super::error::with_copy_context;
use super::traversal::copy_dir_iterative;

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
    let mut stats = LocalCopyDirStats::default();
    let source_result = absolute_path(src);
    #[cfg(coverage)]
    let source_result = if crate::local::internal::coverage_fault::is_enabled(
        "copy-source-absolute",
    ) {
        Err(Error::from_raw_os_error(libc::EIO))
    } else {
        source_result
    };
    let src = with_copy_context(
        source_result,
        LocalCopyDirStage::InspectSource,
        src,
        dst,
        &stats,
    )?;
    let destination_result = if dst.as_os_str().is_empty() {
        Ok(dst.to_path_buf())
    } else {
        absolute_path(dst)
    };
    #[cfg(coverage)]
    let destination_result =
        if crate::local::internal::coverage_fault::is_enabled(
            "copy-destination-absolute",
        ) {
            Err(Error::from_raw_os_error(libc::EIO))
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
    copy_dir_iterative(&src, &dst, options, &destination_root, &mut stats)?;
    Ok(stats)
}
