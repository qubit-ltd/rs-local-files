// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Structured error construction for recursive directory copies.
// qubit-style: allow coverage-cfg

#[cfg(not(coverage))]
use std::io::ErrorKind;
use std::io::{
    Error,
    Result,
};
use std::path::Path;

use crate::{
    LocalCopyDirError,
    LocalCopyDirStage,
    LocalCopyDirStats,
};

use crate::local::internal::StagedFile;

/// Result type used by recursive directory-copy internals.
pub(super) type CopyDirResult<T> = std::result::Result<T, LocalCopyDirError>;

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
pub(super) fn copy_dir_error(
    stage: LocalCopyDirStage,
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
    source: Error,
) -> LocalCopyDirError {
    LocalCopyDirError::new(
        stage,
        src.to_path_buf(),
        dst.to_path_buf(),
        *stats,
        source,
    )
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
    copy_dir_error(stage, src, dst, stats, source)
        .with_staging_context(temporary_path, cleanup_error)
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
#[cfg(not(coverage))]
pub(super) fn record_created_directory(
    stats: &mut LocalCopyDirStats,
) -> Result<()> {
    stats.directories = checked_add(stats.directories, 1, "directories")?;
    Ok(())
}

/// Records one skipped destination file.
#[cfg(not(coverage))]
pub(super) fn record_skipped_file(stats: &mut LocalCopyDirStats) -> Result<()> {
    stats.skipped = checked_add(stats.skipped, 1, "skipped")?;
    Ok(())
}

/// Atomically records one committed file and its copied byte count.
#[cfg(not(coverage))]
pub(super) fn record_copied_file(
    stats: &mut LocalCopyDirStats,
    bytes: u64,
) -> Result<()> {
    let files = checked_add(stats.files, 1, "files")?;
    let bytes = checked_add(stats.bytes, bytes, "bytes")?;
    stats.files = files;
    stats.bytes = bytes;
    Ok(())
}

/// Records one created directory during finite-fixture coverage collection.
#[cfg(coverage)]
pub(super) fn record_created_directory(
    stats: &mut LocalCopyDirStats,
) -> Result<()> {
    stats.directories += 1;
    Ok(())
}

/// Records one skipped file during finite-fixture coverage collection.
#[cfg(coverage)]
pub(super) fn record_skipped_file(stats: &mut LocalCopyDirStats) -> Result<()> {
    stats.skipped += 1;
    Ok(())
}

/// Records one copied file during finite-fixture coverage collection.
#[cfg(coverage)]
pub(super) fn record_copied_file(
    stats: &mut LocalCopyDirStats,
    bytes: u64,
) -> Result<()> {
    stats.files += 1;
    stats.bytes += bytes;
    Ok(())
}

#[cfg(not(coverage))]
fn checked_add(current: u64, amount: u64, field: &str) -> Result<u64> {
    match current.checked_add(amount) {
        Some(value) => Ok(value),
        None => Err(Error::new(
            ErrorKind::InvalidData,
            format!("recursive-copy {field} statistics overflow"),
        )),
    }
}
