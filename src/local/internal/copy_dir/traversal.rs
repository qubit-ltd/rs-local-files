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
use std::time::Instant;

use qubit_budget::BudgetError;
use qubit_budget::ResourceBudget;
use qubit_budget::ResourcePool;

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
use crate::LocalResourceKind;
use crate::local::CopyDestinationAction;

struct CopyBudget {
    entries: Option<ResourceBudget<LocalResourceKind, usize>>,
    bytes: Option<ResourceBudget<LocalResourceKind, u64>>,
    open_directories: Option<ResourcePool<LocalResourceKind, usize>>,
    max_depth: Option<usize>,
    deadline: Option<Instant>,
}

impl CopyBudget {
    fn new(options: LocalCopyDirOptions) -> Self {
        Self {
            entries: options.max_entries().map(|limit| {
                ResourceBudget::new(LocalResourceKind::Entry, limit)
            }),
            bytes: options.max_bytes().map(|limit| {
                ResourceBudget::new(LocalResourceKind::CopiedBytes, limit)
            }),
            open_directories: options.max_open_directories().map(|limit| {
                ResourcePool::new(LocalResourceKind::OpenDirectory, limit)
            }),
            max_depth: options.max_depth(),
            deadline: options
                .deadline()
                .map(|duration| Instant::now() + duration),
        }
    }

    fn check_deadline(&self, path: &Path) -> CopyDirResult<()> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(copy_dir_error(
                LocalCopyDirStage::ReadSourceDirectory,
                path,
                path,
                &LocalCopyDirStats::default(),
                Error::new(ErrorKind::TimedOut, "local copy deadline exceeded"),
            ));
        }
        Ok(())
    }

    fn entry(
        &mut self,
        path: &Path,
        stats: &LocalCopyDirStats,
    ) -> CopyDirResult<()> {
        if let Some(budget) = self.entries.as_mut() {
            budget
                .try_consume(1)
                .map_err(|error| budget_error(path, stats, error))?;
        }
        Ok(())
    }

    fn bytes(
        &mut self,
        path: &Path,
        stats: &LocalCopyDirStats,
        count: u64,
    ) -> CopyDirResult<()> {
        if let Some(budget) = self.bytes.as_mut() {
            budget
                .try_consume(count)
                .map_err(|error| budget_error(path, stats, error))?;
        }
        Ok(())
    }
}

fn budget_error<Q: Copy + std::fmt::Debug>(
    path: &Path,
    stats: &LocalCopyDirStats,
    error: BudgetError<LocalResourceKind, Q>,
) -> crate::LocalCopyDirError {
    let _ = error;
    copy_dir_error(
        LocalCopyDirStage::UpdateStatistics,
        path,
        path,
        stats,
        Error::new(
            ErrorKind::QuotaExceeded,
            "local copy resource budget exceeded",
        ),
    )
}

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
        budget.check_deadline(
            frames.last().expect("non-empty traversal stack").src(),
        )?;
        let entry = frames
            .last_mut()
            .expect("non-empty traversal stack should have a frame")
            .next_entry();
        let Some(entry) = entry else {
            let completed = frames
                .pop()
                .expect("non-empty traversal stack should have a frame");
            if let Some(pool) = budget.open_directories.as_mut() {
                pool.release(1)
                    .expect("completed directory held one budget slot");
            }
            let _ = active_sources.remove(completed.source_identity());
            if options.preserves_permissions() {
                with_copy_context(
                    fs::set_permissions(
                        completed.dst(),
                        completed.source_permissions().clone(),
                    ),
                    LocalCopyDirStage::PreservePermissions,
                    completed.src(),
                    completed.dst(),
                    stats,
                )?;
            }
            continue;
        };

        let current = frames
            .last()
            .expect("active traversal should retain its current frame");
        let entry = with_copy_context(
            entry,
            LocalCopyDirStage::ReadSourceDirectory,
            current.src(),
            current.dst(),
            stats,
        )?;
        budget.entry(&entry.path(), stats)?;
        let source_path = entry.path();
        let destination_path = current.dst().join(entry.file_name());
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
                && symlink_target_is_directory(
                    &source_path,
                    &destination_path,
                    stats,
                    scope_root,
                )?
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
                super::staged_copy::copy_symlink_with_options(
                    &source_path,
                    &destination_path,
                    options,
                    stats,
                )?;
            }
        } else {
            let metadata = with_copy_context(
                fs::metadata(&source_path),
                LocalCopyDirStage::InspectSourceEntry,
                &source_path,
                &destination_path,
                stats,
            )?;
            budget.bytes(&source_path, stats, metadata.len())?;
            copy_file_with_options(
                &source_path,
                &destination_path,
                options,
                stats,
            )?;
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
    budget.check_deadline(src)?;
    if budget.max_depth.is_some_and(|max_depth| depth > max_depth) {
        return Err(copy_dir_error(
            LocalCopyDirStage::InspectSource,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::QuotaExceeded,
                "local copy depth budget exceeded",
            ),
        ));
    }
    let (source_metadata, source_identity) = with_copy_context(
        inspect_copy_source_directory(
            src,
            options.symlink_policy(),
            destination_root,
        ),
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
        ensure_copy_destination_dir(
            dst,
            options.conflict_policy(),
            options.type_conflict_policy(),
        ),
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
    let entries = with_copy_context(
        fs::read_dir(src),
        LocalCopyDirStage::ReadSourceDirectory,
        src,
        dst,
        stats,
    )?;
    if let Some(pool) = budget.open_directories.as_mut() {
        pool.try_acquire(1)
            .map_err(|error| budget_error(src, stats, error))?;
    }
    let _ = active_sources.insert(source_identity.clone());
    Ok(Some(CopyDirFrame::new(
        src.to_path_buf(),
        dst.to_path_buf(),
        source_identity,
        source_metadata.permissions(),
        entries,
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
                    format!(
                        "followed symbolic-link directory escaped copy scope: {}",
                        src.display()
                    ),
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
                format!(
                    "unsupported symbolic link target type: {}",
                    src.display(),
                ),
            ),
        ))
    }
}
