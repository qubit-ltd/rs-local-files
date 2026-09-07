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
    fn frame_context(&self, frame: &Self::Frame) -> CopyTreeFrameContext;

    /// Reads one entry, returning `None` at directory exhaustion.
    ///
    /// Errors retain the directory coordinates and completed statistics.
    /// This method does not charge entry budget or publish destinations.
    fn next_entry(
        &mut self,
        frame: &mut Self::Frame,
        stats: &LocalCopyDirStats,
    ) -> Result<Option<Self::Entry>, LocalCopyDirError>;

    /// Constructs child paths and depth once, without filesystem mutation.
    fn child_context(&self, frame: &CopyTreeFrameContext, entry: &Self::Entry) -> CopyTreeFrameContext;

    /// Processes an already-budgeted child using its precomputed coordinates.
    ///
    /// Returns `None` for a completed or skipped entry, or `Some(frame)` to
    /// descend. Backends must not charge the child entry a second time.
    /// Errors include all proven publication effects and updated statistics;
    /// unreturned readers and permits are dropped before propagating failure.
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
    fn finish_frame(&mut self, frame: Self::Frame, stats: &mut LocalCopyDirStats) -> Result<(), LocalCopyDirError>;

    /// Wraps a scheduler failure with its coordinates and completed statistics.
    /// This conversion must neither mutate destinations nor advance accounting.
    fn error(
        &self,
        stage: LocalCopyDirStage,
        context: &CopyTreeFrameContext,
        stats: &LocalCopyDirStats,
        source_error: io::Error,
    ) -> LocalCopyDirError;
}
