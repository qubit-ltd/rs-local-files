// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured error construction for recursive directory copies.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::io::Error;
use std::io::Result;
use std::path::Path;

use super::copy_dir_result::CopyDirResult;
use super::statistics_overflow::byte_statistics_overflow_error;
use super::statistics_overflow::directory_statistics_overflow_error;
use super::statistics_overflow::file_statistics_overflow_error;
use super::statistics_overflow::overwritten_statistics_overflow_error;
use super::statistics_overflow::skipped_statistics_overflow_error;
use crate::LocalCopyDirError;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::local::internal::StagedFile;

/// Builds a recursive-copy error from the current entry and statistics.
///
/// # Parameters
///
/// * `stage` - Stage at which the copy failed.
/// * `src` - Source entry being processed.
/// * `dst` - Destination entry being processed.
/// * `stats` - Statistics accumulated before the failure.
/// * `source` - Native I/O error that caused the failure.
///
/// # Returns
///
/// A structured recursive-copy error retaining the native source error.
#[inline]
pub(super) fn copy_dir_error(
    stage: LocalCopyDirStage,
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
    source: Error,
) -> LocalCopyDirError {
    LocalCopyDirError::new(stage, src.to_path_buf(), dst.to_path_buf(), *stats, source)
}

/// Builds a recursive-copy error and attempts explicit staging cleanup.
///
/// # Parameters
///
/// * `stage` - Stage at which the copy failed.
/// * `src` - Source entry being processed.
/// * `dst` - Destination entry being processed.
/// * `stats` - Statistics accumulated before the failure.
/// * `source` - Primary native I/O error.
/// * `staged_file` - Armed staging file to clean up.
///
/// # Returns
///
/// A structured error retaining primary and secondary cleanup context.
#[inline]
pub(super) fn copy_dir_error_with_staging(
    stage: LocalCopyDirStage,
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
    source: Error,
    staged_file: &mut StagedFile,
) -> LocalCopyDirError {
    let temporary_path = staged_file.path().to_path_buf();
    let cleanup_error = staged_file.cleanup().err();
    copy_dir_error(stage, src, dst, stats, source).with_staging_context(temporary_path, cleanup_error)
}

/// Adds recursive-copy context to a native I/O result.
///
/// # Parameters
///
/// * `result` - Native I/O result to convert.
/// * `stage` - Copy stage associated with the operation.
/// * `src` - Source path associated with the operation.
/// * `dst` - Destination path associated with the operation.
/// * `stats` - Statistics accumulated before the operation.
///
/// # Returns
///
/// The successful value or a structured recursive-copy error.
#[inline(always)]
pub(super) fn with_copy_context<T>(
    result: Result<T>,
    stage: LocalCopyDirStage,
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
) -> CopyDirResult<T> {
    result.map_err(|error| copy_dir_error(stage, src, dst, stats, error))
}

/// Records one newly created destination directory.
pub(super) fn record_created_directory(stats: &mut LocalCopyDirStats) -> Result<()> {
    #[cfg(feature = "internal-test-support")]
    let directories = if crate::local::internal::test_support::is_enabled("copy-stats-directories") {
        None
    } else {
        stats.directories.checked_add(1)
    };
    #[cfg(not(feature = "internal-test-support"))]
    let directories = stats.directories.checked_add(1);
    match directories {
        Some(directories) => {
            stats.directories = directories;
            Ok(())
        }
        None => Err(directory_statistics_overflow_error()),
    }
}

/// Records one skipped destination file.
pub(super) fn record_skipped_file(stats: &mut LocalCopyDirStats) -> Result<()> {
    #[cfg(feature = "internal-test-support")]
    let skipped = if crate::local::internal::test_support::is_enabled("copy-stats-skipped") {
        None
    } else {
        stats.skipped.checked_add(1)
    };
    #[cfg(not(feature = "internal-test-support"))]
    let skipped = stats.skipped.checked_add(1);
    match skipped {
        Some(skipped) => {
            stats.skipped = skipped;
            Ok(())
        }
        None => Err(skipped_statistics_overflow_error()),
    }
}

/// Records one destination entry replaced by a completed copy.
pub(super) fn record_overwritten_entry(stats: &mut LocalCopyDirStats) -> Result<()> {
    #[cfg(feature = "internal-test-support")]
    let overwritten = if crate::local::internal::test_support::is_enabled("copy-stats-overwritten") {
        None
    } else {
        stats.overwritten.checked_add(1)
    };
    #[cfg(not(feature = "internal-test-support"))]
    let overwritten = stats.overwritten.checked_add(1);
    match overwritten {
        Some(overwritten) => {
            stats.overwritten = overwritten;
            Ok(())
        }
        None => Err(overwritten_statistics_overflow_error()),
    }
}

/// Atomically records one committed file and its copied byte count.
pub(super) fn record_copied_file(stats: &mut LocalCopyDirStats, bytes: u64) -> Result<()> {
    #[cfg(feature = "internal-test-support")]
    let files = if crate::local::internal::test_support::is_enabled("copy-stats-files") {
        None
    } else {
        stats.files.checked_add(1)
    };
    #[cfg(not(feature = "internal-test-support"))]
    let files = stats.files.checked_add(1);
    let files = match files {
        Some(files) => files,
        None => return Err(file_statistics_overflow_error()),
    };
    #[cfg(feature = "internal-test-support")]
    let total_bytes = if crate::local::internal::test_support::is_enabled("copy-stats-bytes") {
        None
    } else {
        stats.bytes.checked_add(bytes)
    };
    #[cfg(not(feature = "internal-test-support"))]
    let total_bytes = stats.bytes.checked_add(bytes);
    match total_bytes {
        Some(total_bytes) => {
            stats.files = files;
            stats.bytes = total_bytes;
            Ok(())
        }
        None => Err(byte_statistics_overflow_error()),
    }
}
