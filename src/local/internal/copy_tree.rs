// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Shared depth-first scheduling for native directory-tree copies.
//!
//! The Host and Rooted backends keep their platform-specific authority and
//! entry operations, while this module owns the traversal stack, deadline,
//! entry and depth checks, and post-order frame completion.
// qubit-style: allow multiple-public-types

use std::path::PathBuf;

use super::CopyBudget;
use crate::LocalCopyDirError;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;

/// The paths and depth associated with one active directory frame.
#[derive(Clone, Debug)]
pub(crate) struct CopyTreeFrameContext {
    pub(crate) source: PathBuf,
    pub(crate) destination: PathBuf,
    pub(crate) depth: usize,
}

/// Platform-specific operations used by the shared copy scheduler.
pub(crate) trait CopyTreeBackend {
    type Frame;
    type Entry;

    fn frame_context(&self, frame: &Self::Frame) -> CopyTreeFrameContext;

    fn next_entry(
        &mut self,
        frame: &mut Self::Frame,
        stats: &LocalCopyDirStats,
    ) -> Result<Option<Self::Entry>, LocalCopyDirError>;

    fn child_context(&self, frame: &CopyTreeFrameContext, entry: &Self::Entry) -> CopyTreeFrameContext;

    fn process_entry(
        &mut self,
        entry: Self::Entry,
        frame: &CopyTreeFrameContext,
        frames: &mut Vec<Self::Frame>,
        stats: &mut LocalCopyDirStats,
        budget: &mut CopyBudget,
    ) -> Result<(), LocalCopyDirError>;

    fn finish_frame(&mut self, frame: Self::Frame, stats: &mut LocalCopyDirStats) -> Result<(), LocalCopyDirError>;

    fn error(
        &self,
        stage: LocalCopyDirStage,
        context: &CopyTreeFrameContext,
        stats: &LocalCopyDirStats,
        source_error: std::io::Error,
    ) -> LocalCopyDirError;
}

/// Runs a lazy depth-first copy using a platform-specific backend.
pub(crate) fn copy_tree<B: CopyTreeBackend>(
    backend: &mut B,
    root_frame: B::Frame,
    stats: &mut LocalCopyDirStats,
    budget: &mut CopyBudget,
) -> Result<(), LocalCopyDirError> {
    let mut frames = vec![root_frame];
    while !frames.is_empty() {
        let current = backend.frame_context(frames.last().expect("non-empty frame stack"));
        if let Err(source_error) = budget.check_deadline() {
            return Err(backend.error(LocalCopyDirStage::ReadSourceDirectory, &current, stats, source_error));
        }

        let next = backend.next_entry(frames.last_mut().expect("non-empty frame stack"), stats)?;
        let Some(entry) = next else {
            let frame = frames.pop().expect("non-empty frame stack");
            backend.finish_frame(frame, stats)?;
            continue;
        };

        let child = backend.child_context(&current, &entry);
        if let Err(source_error) = budget.check_depth(child.depth) {
            return Err(backend.error(LocalCopyDirStage::InspectSourceEntry, &child, stats, source_error));
        }
        if let Err(source_error) = budget.charge_entry() {
            return Err(backend.error(LocalCopyDirStage::UpdateStatistics, &child, stats, source_error));
        }
        backend.process_entry(entry, &current, &mut frames, stats, budget)?;
    }
    Ok(())
}
