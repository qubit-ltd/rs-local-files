// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host-specific implementation of the iterative copy-tree backend.

use std::collections::HashSet;
use std::fs;
use std::io::Error;
use std::path::PathBuf;

use super::super::directory_identity::DirectoryIdentity;
use super::copy_dir_frame::CopyDirFrame;
use super::error::copy_dir_error;
use super::error::with_copy_context;
use super::host_copy_backend_ops::enter_copy_directory;
use super::host_copy_backend_ops::symlink_target_is_directory;
use super::staged_copy::copy_file_with_options;
use crate::LocalCopyDirError;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::local::CopyBudget;
use crate::local::internal::CopyMetadataEffect;
use crate::local::internal::CopyTreeBackend;
use crate::local::internal::CopyTreeFrameContext;

/// Host-specific copy operations and active directory identities.
pub(super) struct HostCopyBackend {
    /// Fixed copy policy for the operation.
    pub(super) options: LocalCopyDirOptions,
    /// Canonical target used to reject traversal into the destination.
    pub(super) destination_root: PathBuf,
    /// Optional Host containment boundary for followed directory links.
    pub(super) scope_root: Option<PathBuf>,
    /// Native identities of the currently active directory ancestors.
    pub(super) active_sources: HashSet<DirectoryIdentity>,
}

impl CopyTreeBackend for HostCopyBackend {
    /// Owns the active directory reader and its managed-resource permit.
    type Frame = CopyDirFrame;
    /// One lazily enumerated child before scheduler budget checks.
    type Entry = fs::DirEntry;

    /// Clones frame coordinates for diagnostics without opening native handles.
    ///
    /// # Parameters
    ///
    /// * `frame` - Active frame whose paths and depth are copied.
    ///
    /// # Returns
    ///
    /// Owned source and destination paths with the frame's logical depth.
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
    /// # Parameters
    ///
    /// * `frame` - Active directory reader to advance.
    /// * `stats` - Read-only completed-copy statistics attached to errors.
    ///
    /// # Returns
    ///
    /// `Some(entry)` contains the next child; `None` means the directory is
    /// exhausted.
    ///
    /// # Errors
    ///
    /// Preserves current statistics and directory coordinates on reader
    /// failure.
    fn next_entry(
        &mut self,
        frame: &mut Self::Frame,
        stats: &LocalCopyDirStats,
    ) -> Result<Option<Self::Entry>, LocalCopyDirError> {
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
    ///
    /// # Parameters
    ///
    /// * `frame` - Parent coordinates used to derive destination and depth.
    /// * `entry` - Child entry whose source and name provide its coordinates.
    ///
    /// # Returns
    ///
    /// The child's source path, destination path, and logical depth.
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
    ///
    /// # Parameters
    ///
    /// * `entry` - Already-admitted native child entry.
    /// * `frame` - Precomputed child coordinates and depth.
    /// * `stats` - Mutable accumulator for completed destination effects.
    /// * `budget` - Shared byte and directory-resource budget.
    ///
    /// # Returns
    ///
    /// `Some(frame)` transfers an opened directory to the scheduler;
    /// `None` means the child was copied or skipped.
    fn process_entry(
        &mut self,
        entry: Self::Entry,
        frame: &CopyTreeFrameContext,
        stats: &mut LocalCopyDirStats,
        budget: &mut CopyBudget,
    ) -> Result<Option<Self::Frame>, LocalCopyDirError> {
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
                super::staged_copy::copy_symlink_with_options(
                    source_path,
                    destination_path,
                    self.options,
                    stats,
                    budget,
                )?;
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
    ///
    /// # Parameters
    ///
    /// * `frame` - Completed directory frame whose permissions are restored.
    /// * `stats` - Mutable accumulator used in any reported failure.
    ///
    /// # Returns
    ///
    /// `Ok(())` after permissions are restored or when preservation is
    /// disabled.
    fn finish_frame(&mut self, frame: Self::Frame, stats: &mut LocalCopyDirStats) -> Result<(), LocalCopyDirError> {
        let _ = self.active_sources.remove(frame.source_identity());
        if self.options.preserves_permissions() {
            let previous_effect = stats.metadata_effect;
            stats.metadata_effect = CopyMetadataEffect::Indeterminate;
            if let Err(source_error) = fs::set_permissions(frame.dst(), frame.source_permissions().clone()) {
                return Err(copy_dir_error(
                    LocalCopyDirStage::PreservePermissions,
                    frame.src(),
                    frame.dst(),
                    stats,
                    source_error,
                ));
            }
            if previous_effect != CopyMetadataEffect::Indeterminate {
                stats.metadata_effect = CopyMetadataEffect::Applied;
            }
        }
        Ok(())
    }

    /// Attaches scheduler failure coordinates and a snapshot of partial
    /// effects.
    ///
    /// # Parameters
    ///
    /// * `stage` - Scheduler stage at which the failure occurred.
    /// * `context` - Source and destination coordinates for the failure.
    /// * `stats` - Read-only snapshot of completed copy effects.
    /// * `source_error` - Native error being wrapped.
    ///
    /// # Returns
    ///
    /// A copy error carrying the stage, paths, completed statistics, and cause.
    fn error(
        &self,
        stage: LocalCopyDirStage,
        context: &CopyTreeFrameContext,
        stats: &LocalCopyDirStats,
        source_error: Error,
    ) -> LocalCopyDirError {
        copy_dir_error(stage, &context.source, &context.destination, stats, source_error)
    }
}
