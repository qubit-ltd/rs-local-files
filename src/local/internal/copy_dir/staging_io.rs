// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Live-descriptor I/O normalization for recursive-copy staging.
// qubit-style: allow source-test-pair
// qubit-style: allow coverage-cfg
// Public APIs retain both descriptors, so post-open copy and permission
// failures cannot be induced deterministically by portable fixtures.

use std::fs::{File, Metadata};
use std::io;
use std::path::Path;

use crate::local::internal::StagedFile;
use crate::{LocalCopyDirStage, LocalCopyDirStats};

use super::copy_dir_result::CopyDirResult;
use super::error::copy_dir_error_with_staging;

/// Copies one open source handle into an armed staging file.
///
/// # Parameters
///
/// * `src` - Source path included in a structured failure.
/// * `dst` - Destination path included in a structured failure.
/// * `stats` - Recursive-copy statistics snapshot attached to a failure.
/// * `source_file` - Open source descriptor positioned at its beginning.
/// * `staged_file` - Armed destination staging file.
///
/// # Returns
///
/// Number of bytes copied into staging.
///
/// # Errors
///
/// Returns a structured copy error and attempts staging cleanup when descriptor
/// I/O fails.
pub(super) fn copy_into_staging(
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
    source_file: &mut File,
    staged_file: &mut StagedFile,
) -> CopyDirResult<u64> {
    #[cfg(coverage)]
    let result = if crate::local::internal::coverage_fault::is_enabled("copy-staging-copy")
        || crate::local::internal::coverage_fault::is_enabled("copy-staging-copy-cleanup")
        || crate::local::internal::coverage_fault::take_on_nth("copy-staging-copy-second", 2)
    {
        Err(io::Error::from_raw_os_error(libc::EIO))
    } else {
        io::copy(source_file, staged_file.file_mut())
    };
    #[cfg(not(coverage))]
    let result = io::copy(source_file, staged_file.file_mut());
    match result {
        Ok(copied) => Ok(copied),
        Err(source) => Err(copy_dir_error_with_staging(
            LocalCopyDirStage::CopyFileContents,
            src,
            dst,
            stats,
            source,
            staged_file,
        )),
    }
}

/// Applies authoritative source permissions to an armed staging file.
///
/// # Parameters
///
/// * `src` - Source path included in a structured failure.
/// * `dst` - Destination path included in a structured failure.
/// * `source_metadata` - Metadata read from the copied source handle.
/// * `stats` - Recursive-copy statistics snapshot attached to a failure.
/// * `staged_file` - Armed staging file receiving the permissions.
///
/// # Errors
///
/// Returns a structured copy error and attempts staging cleanup when applying
/// permissions fails.
pub(super) fn preserve_staged_permissions(
    src: &Path,
    dst: &Path,
    source_metadata: &Metadata,
    stats: &LocalCopyDirStats,
    staged_file: &mut StagedFile,
) -> CopyDirResult<()> {
    #[cfg(coverage)]
    let result = if crate::local::internal::coverage_fault::is_enabled("copy-staging-permissions") {
        Err(io::Error::from_raw_os_error(libc::EIO))
    } else {
        staged_file
            .file()
            .set_permissions(source_metadata.permissions())
    };
    #[cfg(not(coverage))]
    let result = staged_file
        .file()
        .set_permissions(source_metadata.permissions());
    if let Err(source) = result {
        return Err(copy_dir_error_with_staging(
            LocalCopyDirStage::PreservePermissions,
            src,
            dst,
            stats,
            source,
            staged_file,
        ));
    }
    Ok(())
}
