// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared depth-first scheduling for native directory-tree copies.
//!
//! The Host and Rooted backends keep their platform-specific authority and
//! entry operations, while this module owns the traversal stack, deadline,
//! entry and depth checks, and post-order frame completion.

use super::CopyBudget;
use super::CopyTreeBackend;
use crate::LocalCopyDirError;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;

/// Runs a lazy depth-first copy using a platform-specific backend.
///
/// The caller supplies an opened root frame and owns root-entry charging; this
/// scheduler charges each descendant exactly once before backend processing.
/// Only this function changes the stack. A backend may return one child frame.
///
/// # Type Parameters
///
/// - `B`: Backend that supplies platform-specific entry operations and owns the
///   frame and entry types used during traversal.
///
/// # Parameters
///
/// - `backend`: Mutable backend used to read, process, and finish directory
///   frames.
/// - `root_frame`: Already-opened root directory frame; its entry is charged by
///   the caller.
/// - `stats`: Accumulated copy statistics, updated as backend operations
///   complete.
/// - `budget`: Mutable limits and deadline state used to check and charge
///   descendant work.
///
/// # Errors
///
/// Returns the backend failure, preserving already-applied statistics, or a
/// depth, entry, or deadline error before processing the rejected descendant.
/// On any error, all stacked frames are dropped and release their readers and
/// permits; completed destination changes are not rolled back.
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
            #[cfg(test)]
            crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
                crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::HostCopyBeforeDirectoryMetadata,
            );
            if let Err(source_error) = budget.check_deadline() {
                return Err(backend.error(LocalCopyDirStage::PreservePermissions, &current, stats, source_error));
            }
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
        if let Some(frame) = backend.process_entry(entry, &child, stats, budget)? {
            frames.push(frame);
        }
    }
    Ok(())
}
