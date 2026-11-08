// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Platform operations used by the shared directory-copy scheduler.

use std::io;

use super::CopyBudget;
use super::CopyTreeFrameContext;
use crate::LocalCopyDirError;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;

/// Implements native entry operations without access to the traversal stack.
///
/// The scheduler checks deadlines, depth, and entry count before calling
/// `process_entry`. Backends charge actual copied bytes through `CopyBudget`,
/// acquire directory permits before opening readers, and retain permits in
/// frames. Statistics describe completed effects, including effects preceding
/// failure; neither errors nor frame destruction roll back publication.
pub(crate) trait CopyTreeBackend {
    /// Owned reader and permit released together when a frame is dropped.
    type Frame;
    /// One lazily read native entry, consumed at most once by `process_entry`.
    type Entry;

    /// Copies the active frame's coordinates without changing its reader.
    ///
    /// # Parameters
    ///
    /// - `frame`: Active directory frame whose paths and depth are copied.
    ///
    /// # Returns
    ///
    /// The frame's source and destination coordinates and logical depth.
    #[must_use]
    fn frame_context(&self, frame: &Self::Frame) -> CopyTreeFrameContext;

    /// Reads one entry, returning `None` at directory exhaustion.
    ///
    /// # Parameters
    ///
    /// - `frame`: Active directory reader advanced by this operation.
    /// - `stats`: Completed copy effects included in any read error.
    ///
    /// # Returns
    ///
    /// The next lazy entry, or `None` when the directory is exhausted.
    ///
    /// # Errors
    ///
    /// Errors retain the directory coordinates and completed statistics.
    /// This method does not charge entry budget or publish destinations.
    fn next_entry(
        &mut self,
        frame: &mut Self::Frame,
        stats: &LocalCopyDirStats,
    ) -> Result<Option<Self::Entry>, LocalCopyDirError>;

    /// Constructs child paths and depth once, without filesystem mutation.
    ///
    /// # Parameters
    ///
    /// - `frame`: Parent coordinates used to derive the child's paths/depth.
    /// - `entry`: Native entry whose child coordinates are being constructed.
    ///
    /// # Returns
    ///
    /// The child's source and destination coordinates and logical depth.
    #[must_use]
    fn child_context(&self, frame: &CopyTreeFrameContext, entry: &Self::Entry) -> CopyTreeFrameContext;

    /// Processes an already-budgeted child using its precomputed coordinates.
    ///
    /// Returns `None` for a completed or skipped entry, or `Some(frame)` to
    /// descend. Backends must not charge the child entry a second time.
    /// Errors include all proven publication effects and updated statistics;
    /// unreturned readers and permits are dropped before propagating failure.
    ///
    /// # Parameters
    ///
    /// - `entry`: Child entry already charged by the scheduler.
    /// - `child`: Precomputed paths and depth for that entry.
    /// - `stats`: Mutable completed-effect counters updated during processing.
    /// - `budget`: Shared byte and directory-resource accounting state.
    ///
    /// # Returns
    ///
    /// `Some(frame)` when the child is a directory to descend into; otherwise
    /// `None` after the entry is complete or skipped.
    ///
    /// # Errors
    ///
    /// Returns the backend failure with publication effects and statistics
    /// preserved; the scheduler does not charge the entry again.
    fn process_entry(
        &mut self,
        entry: Self::Entry,
        child: &CopyTreeFrameContext,
        stats: &mut LocalCopyDirStats,
        budget: &mut CopyBudget,
    ) -> Result<Option<Self::Frame>, LocalCopyDirError>;

    /// Completes a directory after its descendants, consuming its resources.
    ///
    /// Metadata-preservation failures retain partial effects and release the
    /// consumed frame. The scheduler then drops every remaining ancestor.
    ///
    /// # Parameters
    ///
    /// - `frame`: Completed directory frame whose resources are consumed.
    /// - `stats`: Completed effects retained if metadata preservation fails.
    ///
    /// # Returns
    ///
    /// `Ok(())` after directory completion.
    ///
    /// # Errors
    ///
    /// Returns a metadata-preservation error with completed effects retained.
    fn finish_frame(&mut self, frame: Self::Frame, stats: &mut LocalCopyDirStats) -> Result<(), LocalCopyDirError>;

    /// Wraps a scheduler failure with its coordinates and completed statistics.
    /// This conversion must neither mutate destinations nor advance accounting.
    ///
    /// # Parameters
    ///
    /// - `stage`: Scheduler stage at which the source error occurred.
    /// - `context`: Source/destination coordinates and depth for the failure.
    /// - `stats`: Completed copy effects to include in the wrapped error.
    /// - `source_error`: Underlying scheduler or I/O failure.
    ///
    /// # Returns
    ///
    /// A copy error carrying the stage, coordinates, statistics, and cause.
    #[must_use]
    fn error(
        &self,
        stage: LocalCopyDirStage,
        context: &CopyTreeFrameContext,
        stats: &LocalCopyDirStats,
        source_error: io::Error,
    ) -> LocalCopyDirError;
}
