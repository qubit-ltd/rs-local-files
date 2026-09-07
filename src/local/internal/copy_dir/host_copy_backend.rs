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
use std::path::PathBuf;

use super::super::directory_identity::DirectoryIdentity;
use super::copy_dir_frame::CopyDirFrame;
use super::copy_dir_result::CopyDirResult;
use super::destination::ensure_copy_destination_dir;
use super::error::copy_dir_error;
use super::error::record_created_directory;
use super::error::record_overwritten_entry;
use super::error::record_skipped_file;
use super::error::with_copy_context;
use super::source::inspect_copy_source_directory;
use super::staged_copy::copy_file_with_options;
use crate::LocalCopyConflictPolicy;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::local::CopyBudget;
use crate::local::CopyDestinationAction;
use crate::local::internal::CopyTreeBackend;
use crate::local::internal::CopyTreeFrameContext;
use crate::local::internal::copy_tree;

/// Copies one source directory tree without recursive function calls.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Recursive-copy behavior options.
/// * `destination_root` - Canonical destination used for containment checks.
/// * `scope_root` - Optional canonical boundary for followed directory links.
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
    let mut budget = CopyBudget::new(options);
    let mut backend = HostCopyBackend {
        options,
        destination_root: destination_root.to_path_buf(),
        scope_root: scope_root.map(Path::to_path_buf),
        active_sources: HashSet::new(),
    };
    let Some(root_frame) = enter_copy_directory(
        src,
        dst,
        options,
        destination_root,
        &mut backend.active_sources,
        stats,
        &mut budget,
        0,
    )?
    else {
        return Ok(());
    };
    copy_tree(&mut backend, root_frame, stats, &mut budget)
}

/// Host-specific copy operations and active directory identities.
struct HostCopyBackend {
    /// Fixed copy policy for the operation.
    options: LocalCopyDirOptions,
    /// Canonical target used to reject traversal into the destination.
    destination_root: PathBuf,
    /// Optional Host containment boundary for followed directory links.
    scope_root: Option<PathBuf>,
    /// Native identities of the currently active directory ancestors.
    active_sources: HashSet<DirectoryIdentity>,
}

impl CopyTreeBackend for HostCopyBackend {
    /// Owns the active directory reader and its managed-resource permit.
    type Frame = CopyDirFrame;
    /// One lazily enumerated child before scheduler budget checks.
    type Entry = fs::DirEntry;

    /// Clones frame coordinates for diagnostics without opening native handles.
    #[inline]
    fn frame_context(&self, frame: &Self::Frame) -> CopyTreeFrameContext {
        CopyTreeFrameContext {
            source: frame.src().to_path_buf(),
            destination: frame.dst().to_path_buf(),
            depth: frame.depth(),
        }
    }

    /// Reads one child lazily; `None` means the directory is exhausted.
    ///
    /// # Errors
    ///
    /// Preserves current statistics and directory coordinates on reader
    /// failure.
    fn next_entry(
        &mut self,
        frame: &mut Self::Frame,
        stats: &LocalCopyDirStats,
    ) -> Result<Option<Self::Entry>, crate::LocalCopyDirError> {
        let next = frame.next_entry();
        match next {
            Some(entry) => with_copy_context(
                entry,
                LocalCopyDirStage::ReadSourceDirectory,
                frame.src(),
                frame.dst(),
                stats,
            )
            .map(Some),
            None => Ok(None),
        }
    }

    /// Constructs child coordinates once, at one level below the active frame.
    #[inline]
    fn child_context(&self, frame: &CopyTreeFrameContext, entry: &Self::Entry) -> CopyTreeFrameContext {
        CopyTreeFrameContext {
            source: entry.path(),
            destination: frame.destination.join(entry.file_name()),
            depth: frame.depth.saturating_add(1),
        }
    }

    /// Publishes one admitted child or returns its owned directory frame.
    ///
    /// The scheduler has already charged the entry and checked its depth. This
    /// backend charges bytes and acquired handles, and records destination
    /// mutations as they occur. `None` means a leaf was copied or skipped;
    /// `Some` transfers a child reader and permit to the scheduler.
    ///
    /// # Errors
    ///
    /// Returns native or policy failures with all recorded partial effects.
    fn process_entry(
        &mut self,
        entry: Self::Entry,
        frame: &CopyTreeFrameContext,
        stats: &mut LocalCopyDirStats,
        budget: &mut CopyBudget,
    ) -> Result<Option<Self::Frame>, crate::LocalCopyDirError> {
        let source_path = &frame.source;
        let destination_path = &frame.destination;
        let file_type = with_copy_context(
            entry.file_type(),
            LocalCopyDirStage::InspectSourceEntry,
            source_path,
            destination_path,
            stats,
        )?;
        if file_type.is_dir() {
            return enter_copy_directory(
                source_path,
                destination_path,
                self.options,
                &self.destination_root,
                &mut self.active_sources,
                stats,
                budget,
                frame.depth,
            );
        } else if file_type.is_symlink() {
            if self.options.symlink_policy().follows()
                && symlink_target_is_directory(source_path, destination_path, stats, self.scope_root.as_deref())?
            {
                return enter_copy_directory(
                    source_path,
                    destination_path,
                    self.options,
                    &self.destination_root,
                    &mut self.active_sources,
                    stats,
                    budget,
                    frame.depth,
                );
            } else {
                super::staged_copy::copy_symlink_with_options(source_path, destination_path, self.options, stats)?;
            }
        } else {
            copy_file_with_options(source_path, destination_path, self.options, stats, budget)?;
        }
        Ok(None)
    }

    /// Applies final directory permissions after all descendants are processed.
    ///
    /// Consumes the frame, releasing its reader and permit on success or error.
    ///
    /// # Errors
    ///
    /// Reports permission preservation failures with current copy statistics.
    fn finish_frame(
        &mut self,
        frame: Self::Frame,
        stats: &mut LocalCopyDirStats,
    ) -> Result<(), crate::LocalCopyDirError> {
        let _ = self.active_sources.remove(frame.source_identity());
        if self.options.preserves_permissions() {
            with_copy_context(
                fs::set_permissions(frame.dst(), frame.source_permissions().clone()),
                LocalCopyDirStage::PreservePermissions,
                frame.src(),
                frame.dst(),
                stats,
            )?;
        }
        Ok(())
    }

    /// Attaches scheduler failure coordinates and a snapshot of partial
    /// effects.
    fn error(
        &self,
        stage: LocalCopyDirStage,
        context: &CopyTreeFrameContext,
        stats: &LocalCopyDirStats,
        source_error: Error,
    ) -> crate::LocalCopyDirError {
        copy_dir_error(stage, &context.source, &context.destination, stats, source_error)
    }
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
/// * `budget` - Shared deadline, depth, and open-directory budget.
/// * `depth` - Source directory depth, with the copied root at zero.
///
/// # Returns
///
/// `Some` owns a lazy reader and permit for the entered directory. `None` means
/// conflict policy skipped the destination without entering its source reader.
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
    let (action, created) = with_copy_context(
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
    if created {
        with_copy_context(
            record_created_directory(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
    }
    if action == CopyDestinationAction::Replace
        || (action == CopyDestinationAction::Merge && options.conflict_policy() == LocalCopyConflictPolicy::Overwrite)
    {
        with_copy_context(
            record_overwritten_entry(stats),
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
        depth,
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
/// * `stats` - Read-only snapshot attached to a failed inspection.
/// * `scope_root` - Optional canonical boundary for the resolved link target.
///
/// # Returns
///
/// `true` for a directory target and `false` for a regular-file or missing
/// target when scope canonicalization is not required.
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
