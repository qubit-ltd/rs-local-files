// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Facade for the private recursive directory-copy pipeline.
// Private behavior is covered through public integration tests.

use std::path::Path;

use super::copy_dir_result::CopyDirResult;
use super::error::with_copy_context;
use super::host_copy_backend::copy_dir_iterative;
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
#[inline]
pub(crate) fn copy_dir_all_with_paths(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
) -> CopyDirResult<LocalCopyDirStats> {
    copy_dir_all_with_scope(src, dst, options, None)
}

/// Recursively copies a directory tree while constraining followed directory
/// links to a canonical scope root.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Copy behavior options.
/// * `scope_root` - Root that followed directory links must remain within.
///
/// # Returns
/// Exact statistics for copied files, created directories, bytes, and skips.
///
/// # Errors
/// Returns a structured copy error when path resolution, scope
/// canonicalization, traversal, staging, commit, permission preservation, or
/// statistics accounting fails.
#[inline]
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

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use tempfile::tempdir;

    use super::copy_dir_all_with_paths_scoped;
    use crate::LocalCopyDirOptions;
    use crate::LocalSymlinkPolicy;

    #[test]
    fn test_scoped_copy_follows_in_scope_directory_link() {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let linked = directory.path().join("linked");
        let destination = directory.path().join("destination");
        fs::create_dir_all(&source).expect("source directory should be created");
        fs::create_dir(&linked).expect("link target should be created");
        fs::write(linked.join("entry"), b"payload").expect("link target entry should be written");
        symlink("../linked", source.join("link")).expect("in-scope directory link should be created");

        let stats = copy_dir_all_with_paths_scoped(
            &source,
            &destination,
            LocalCopyDirOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
            directory.path(),
        )
        .expect("copy should follow a directory link inside its scope");

        assert_eq!(1, stats.files);
        assert_eq!(b"payload", fs::read(destination.join("link/entry")).unwrap().as_slice());
    }
}
