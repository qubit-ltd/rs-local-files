// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive enumeration and symbolic-link dispatch for directory copies.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::collections::HashSet;
use std::fs;
use std::io::Error;
use std::io::ErrorKind;
use std::path::Path;

use super::super::directory_identity::DirectoryIdentity;
use super::copy_dir_frame::CopyDirFrame;
use super::copy_dir_result::CopyDirResult;
use super::destination::ensure_copy_destination_dir;
use super::error::copy_dir_error;
use super::error::record_created_directory;
use super::error::record_skipped_file;
use super::error::with_copy_context;
use super::source::inspect_copy_source_directory;
use super::staged_copy::copy_file_with_options;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::local::CopyBudget;
use crate::local::CopyDestinationAction;

/// Copies one source directory tree without recursive function calls.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Recursive-copy behavior options.
/// * `destination_root` - Canonical destination used for containment checks.
/// * `stats` - Mutable statistics accumulator.
///
/// # Errors
///
/// Returns a structured error when inspection, traversal, copying, permission
/// preservation, or exact accounting fails.
///
/// # Panics
///
/// Panics if the iterative traversal loses its active frame.
pub(super) fn copy_dir_iterative(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    scope_root: Option<&Path>,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    let mut active_sources = HashSet::new();
    let mut budget = CopyBudget::new(options);
    let Some(root_frame) = enter_copy_directory(
        src,
        dst,
        options,
        destination_root,
        &mut active_sources,
        stats,
        &mut budget,
        0,
    )?
    else {
        return Ok(());
    };
    let mut frames = vec![root_frame];

    while !frames.is_empty() {
        let current_source = frames.last().expect("non-empty traversal stack").src();
        budget.check_deadline().map_err(|source| {
            copy_dir_error(
                LocalCopyDirStage::ReadSourceDirectory,
                current_source,
                current_source,
                stats,
                source,
            )
        })?;
        let entry = frames
            .last_mut()
            .expect("non-empty traversal stack should have a frame")
            .next_entry();
        let Some(entry) = entry else {
            let completed = frames.pop().expect("non-empty traversal stack should have a frame");
            let _ = active_sources.remove(completed.source_identity());
            if options.preserves_permissions() {
                with_copy_context(
                    fs::set_permissions(completed.dst(), completed.source_permissions().clone()),
                    LocalCopyDirStage::PreservePermissions,
                    completed.src(),
                    completed.dst(),
                    stats,
                )?;
            }
            continue;
        };

        let current = frames.last().expect("active traversal should retain its current frame");
        let entry = with_copy_context(
            entry,
            LocalCopyDirStage::ReadSourceDirectory,
            current.src(),
            current.dst(),
            stats,
        )?;
        let source_path = entry.path();
        let destination_path = current.dst().join(entry.file_name());
        budget.check_depth(frames.len()).map_err(|source| {
            copy_dir_error(
                LocalCopyDirStage::InspectSourceEntry,
                &source_path,
                &destination_path,
                stats,
                source,
            )
        })?;
        budget.charge_entry().map_err(|source| {
            copy_dir_error(
                LocalCopyDirStage::UpdateStatistics,
                &source_path,
                &destination_path,
                stats,
                source,
            )
        })?;
        let file_type = with_copy_context(
            entry.file_type(),
            LocalCopyDirStage::InspectSourceEntry,
            &source_path,
            &destination_path,
            stats,
        )?;
        if file_type.is_dir() {
            let frame = enter_copy_directory(
                &source_path,
                &destination_path,
                options,
                destination_root,
                &mut active_sources,
                stats,
                &mut budget,
                frames.len(),
            )?;
            if let Some(frame) = frame {
                frames.push(frame);
            }
        } else if file_type.is_symlink() {
            if options.symlink_policy().follows()
                && symlink_target_is_directory(&source_path, &destination_path, stats, scope_root)?
            {
                let frame = enter_copy_directory(
                    &source_path,
                    &destination_path,
                    options,
                    destination_root,
                    &mut active_sources,
                    stats,
                    &mut budget,
                    frames.len(),
                )?;
                if let Some(frame) = frame {
                    frames.push(frame);
                }
            } else {
                super::staged_copy::copy_symlink_with_options(&source_path, &destination_path, options, stats)?;
            }
        } else {
            copy_file_with_options(&source_path, &destination_path, options, stats, &mut budget)?;
        }
    }
    Ok(())
}

/// Enters one source directory and constructs its traversal frame.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Recursive-copy behavior options.
/// * `destination_root` - Canonical destination used for containment checks.
/// * `active_sources` - Filesystem-object ancestor identities used for cycle
///   detection.
/// * `stats` - Mutable statistics accumulator.
///
/// # Returns
///
/// A lazy traversal frame for the entered directory.
///
/// # Errors
///
/// Returns a structured error when inspection, cycle validation, destination
/// preparation, statistics accounting, or directory enumeration fails.
#[allow(clippy::too_many_arguments)]
fn enter_copy_directory(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    active_sources: &mut HashSet<DirectoryIdentity>,
    stats: &mut LocalCopyDirStats,
    budget: &mut CopyBudget,
    depth: usize,
) -> CopyDirResult<Option<CopyDirFrame>> {
    budget
        .check_deadline()
        .map_err(|source| copy_dir_error(LocalCopyDirStage::InspectSource, src, dst, stats, source))?;
    budget
        .check_depth(depth)
        .map_err(|source| copy_dir_error(LocalCopyDirStage::InspectSource, src, dst, stats, source))?;
    let (source_metadata, source_identity) = with_copy_context(
        inspect_copy_source_directory(src, options.symlink_policy(), destination_root),
        LocalCopyDirStage::InspectSource,
        src,
        dst,
        stats,
    )?;
    if active_sources.contains(&source_identity) {
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
    let action = with_copy_context(
        ensure_copy_destination_dir(dst, options.conflict_policy(), options.type_conflict_policy()),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
    )?;
    if action == CopyDestinationAction::Skip {
        with_copy_context(
            record_skipped_file(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
        return Ok(None);
    }
    if action == CopyDestinationAction::Create {
        with_copy_context(
            record_created_directory(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
    }
    let directory_permit = budget
        .acquire_directory()
        .map_err(|source| copy_dir_error(LocalCopyDirStage::ReadSourceDirectory, src, dst, stats, source))?;
    let entries = with_copy_context(
        fs::read_dir(src),
        LocalCopyDirStage::ReadSourceDirectory,
        src,
        dst,
        stats,
    )?;
    let _ = active_sources.insert(source_identity.clone());
    Ok(Some(CopyDirFrame::new(
        src.to_path_buf(),
        dst.to_path_buf(),
        source_identity,
        source_metadata.permissions(),
        entries,
        directory_permit,
    )))
}

/// Determines whether an allowed symbolic link targets a directory.
///
/// # Parameters
///
/// * `src` - Source symbolic link.
/// * `dst` - Destination path.
/// * `stats` - Mutable statistics accumulator.
///
/// # Returns
///
/// `true` for a directory target and `false` for a regular-file target.
///
/// # Errors
///
/// Returns a structured error when the target cannot be inspected or has an
/// unsupported type.
fn symlink_target_is_directory(
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
    scope_root: Option<&Path>,
) -> CopyDirResult<bool> {
    if let Some(scope_root) = scope_root {
        let target = with_copy_context(
            fs::canonicalize(src),
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
        )?;
        if !target.starts_with(scope_root) {
            return Err(copy_dir_error(
                LocalCopyDirStage::InspectSourceEntry,
                src,
                dst,
                stats,
                Error::new(
                    ErrorKind::InvalidInput,
                    format!("followed symbolic-link directory escaped copy scope: {}", src.display()),
                ),
            ));
        }
    }
    let target_metadata = match with_copy_context(
        fs::metadata(src),
        LocalCopyDirStage::InspectSourceEntry,
        src,
        dst,
        stats,
    ) {
        Ok(metadata) => metadata,
        Err(error) if error.error().kind() == ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    if target_metadata.is_dir() {
        Ok(true)
    } else if target_metadata.is_file() {
        Ok(false)
    } else {
        Err(copy_dir_error(
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::Unsupported,
                format!("unsupported symbolic link target type: {}", src.display(),),
            ),
        ))
    }
}
