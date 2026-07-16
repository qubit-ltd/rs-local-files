// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Recursive enumeration and symbolic-link dispatch for directory copies.

use std::fs;
use std::io::{
    Error,
    ErrorKind,
};
use std::path::{
    Path,
    PathBuf,
};

use crate::{
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
};

use super::destination::ensure_copy_destination_dir;
use super::error::{
    CopyDirResult,
    copy_dir_error,
    record_created_directory,
    with_copy_context,
};
use super::source::inspect_copy_source_directory;
use super::staged_copy::copy_file_with_options;

/// Recursively copies one source directory into one destination directory.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Recursive-copy behavior options.
/// * `destination_root` - Canonical destination used for containment checks.
/// * `active_sources` - Canonical directory stack used for cycle detection.
/// * `stats` - Mutable statistics accumulator.
///
/// # Errors
///
/// Returns a structured error when inspection, traversal, copying, permission
/// preservation, or exact accounting fails.
pub(super) fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    active_sources: &mut Vec<PathBuf>,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    let (source_metadata, canonical_source) = with_copy_context(
        inspect_copy_source_directory(
            src,
            options.follows_symlinks(),
            destination_root,
        ),
        LocalCopyDirStage::InspectSource,
        src,
        dst,
        stats,
    )?;
    if active_sources
        .iter()
        .any(|active_source| active_source == &canonical_source)
    {
        return Err(copy_dir_error(
            LocalCopyDirStage::InspectSource,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::InvalidInput,
                format!("source directory cycle detected: {}", src.display()),
            ),
        ));
    }
    active_sources.push(canonical_source);
    let result = (|| {
        let created = with_copy_context(
            ensure_copy_destination_dir(dst, options.type_conflict_policy()),
            LocalCopyDirStage::PrepareDestination,
            src,
            dst,
            stats,
        )?;
        if created {
            with_copy_context(
                record_created_directory(stats),
                LocalCopyDirStage::UpdateStatistics,
                src,
                dst,
                stats,
            )?;
        }
        let entries = with_copy_context(
            fs::read_dir(src),
            LocalCopyDirStage::ReadSourceDirectory,
            src,
            dst,
            stats,
        )?;
        for entry in entries {
            let entry = with_copy_context(
                entry,
                LocalCopyDirStage::ReadSourceDirectory,
                src,
                dst,
                stats,
            )?;
            let source_path = entry.path();
            let destination_path = dst.join(entry.file_name());
            let file_type = with_copy_context(
                entry.file_type(),
                LocalCopyDirStage::InspectSourceEntry,
                &source_path,
                &destination_path,
                stats,
            )?;
            if file_type.is_symlink() {
                copy_symlink_source(
                    &source_path,
                    &destination_path,
                    options,
                    destination_root,
                    active_sources,
                    stats,
                )?;
            } else if file_type.is_dir() {
                copy_dir_recursive(
                    &source_path,
                    &destination_path,
                    options,
                    destination_root,
                    active_sources,
                    stats,
                )?;
            } else if file_type.is_file() {
                copy_file_with_options(
                    &source_path,
                    &destination_path,
                    options,
                    stats,
                )?;
            } else {
                return Err(copy_dir_error(
                    LocalCopyDirStage::InspectSourceEntry,
                    &source_path,
                    &destination_path,
                    stats,
                    Error::new(
                        ErrorKind::Unsupported,
                        format!(
                            "unsupported source file type: {}",
                            source_path.display(),
                        ),
                    ),
                ));
            }
        }
        if options.preserves_permissions() {
            with_copy_context(
                fs::set_permissions(dst, source_metadata.permissions()),
                LocalCopyDirStage::PreservePermissions,
                src,
                dst,
                stats,
            )
        } else {
            Ok(())
        }
    })();
    let _ = active_sources.pop();
    result
}

/// Copies a symbolic-link source when following is enabled.
///
/// # Parameters
///
/// * `src` - Source symbolic link.
/// * `dst` - Destination path.
/// * `options` - Recursive-copy behavior options.
/// * `destination_root` - Canonical destination used for containment checks.
/// * `active_sources` - Canonical directory stack used for cycle detection.
/// * `stats` - Mutable statistics accumulator.
///
/// # Errors
///
/// Returns a structured error when links are disabled or the target cannot be
/// inspected or copied.
fn copy_symlink_source(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    active_sources: &mut Vec<PathBuf>,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    if !options.follows_symlinks() {
        return Err(copy_dir_error(
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::Unsupported,
                format!("symbolic links are not followed: {}", src.display()),
            ),
        ));
    }
    let target_metadata = with_copy_context(
        fs::metadata(src),
        LocalCopyDirStage::InspectSourceEntry,
        src,
        dst,
        stats,
    )?;
    if target_metadata.is_dir() {
        copy_dir_recursive(
            src,
            dst,
            options,
            destination_root,
            active_sources,
            stats,
        )
    } else if target_metadata.is_file() {
        copy_file_with_options(src, dst, options, stats)
    } else {
        Err(copy_dir_error(
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::Unsupported,
                format!(
                    "unsupported symbolic link target type: {}",
                    src.display(),
                ),
            ),
        ))
    }
}
