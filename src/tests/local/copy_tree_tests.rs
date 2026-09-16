// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scheduler ownership, ordering, and accounting without native I/O noise.

use std::cell::Cell;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;

use qubit_budget::ManagedResourcePermit;

use crate::LocalCopyDirError;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::LocalResourceKind;
use crate::local::CopyBudget;
use crate::local::CopyTreeBackend;
use crate::local::CopyTreeFrameContext;
use crate::local::copy_tree;

struct Frame {
    depth: usize,
    visited: bool,
    live: Rc<Cell<usize>>,
    _permit: Option<ManagedResourcePermit<LocalResourceKind, usize>>,
}

impl Frame {
    /// Retains observable ownership and real directory-budget capacity.
    fn new(depth: usize, live: Rc<Cell<usize>>, budget: &CopyBudget) -> Self {
        let permit = budget
            .acquire_directory()
            .expect("fixture directory capacity should exist");
        live.set(live.get() + 1);
        Self {
            depth,
            visited: false,
            live,
            _permit: permit,
        }
    }
}

impl Drop for Frame {
    /// Counts live frames independently of backend completion callbacks.
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}

struct Backend {
    live: Rc<Cell<usize>>,
    finished: Vec<usize>,
    failure: Option<LocalCopyDirStage>,
}

impl CopyTreeBackend for Backend {
    type Frame = Frame;
    type Entry = ();

    /// Gives each depth a distinct diagnostic path.
    fn frame_context(&self, frame: &Frame) -> CopyTreeFrameContext {
        context(frame.depth)
    }

    /// Emits one child per frame until the terminal directory.
    fn next_entry(&mut self, frame: &mut Frame, stats: &LocalCopyDirStats) -> Result<Option<()>, LocalCopyDirError> {
        if frame.depth == 1 && self.failure == Some(LocalCopyDirStage::ReadSourceDirectory) {
            return Err(self.error(
                LocalCopyDirStage::ReadSourceDirectory,
                &context(frame.depth),
                stats,
                io::ErrorKind::PermissionDenied.into(),
            ));
        }
        if frame.depth == 2 || frame.visited {
            return Ok(None);
        }
        frame.visited = true;
        Ok(Some(()))
    }

    /// Child context must be passed unchanged into processing.
    fn child_context(&self, frame: &CopyTreeFrameContext, _: &()) -> CopyTreeFrameContext {
        context(frame.depth + 1)
    }

    /// Records a completed destination effect before an optional injected
    /// failure.
    fn process_entry(
        &mut self,
        _: (),
        child: &CopyTreeFrameContext,
        stats: &mut LocalCopyDirStats,
        budget: &mut CopyBudget,
    ) -> Result<Option<Frame>, LocalCopyDirError> {
        assert_eq!(context(child.depth).source, child.source);
        stats.directories += 1;
        if child.depth == 2 && self.failure == Some(LocalCopyDirStage::PrepareDestination) {
            return Err(self.error(
                LocalCopyDirStage::PrepareDestination,
                child,
                stats,
                io::ErrorKind::PermissionDenied.into(),
            ));
        }
        Ok(Some(Frame::new(child.depth, Rc::clone(&self.live), budget)))
    }

    /// Records post-order completion or fails while consuming the deepest
    /// frame.
    fn finish_frame(&mut self, frame: Frame, stats: &mut LocalCopyDirStats) -> Result<(), LocalCopyDirError> {
        if frame.depth == 2 && self.failure == Some(LocalCopyDirStage::PreservePermissions) {
            return Err(self.error(
                LocalCopyDirStage::PreservePermissions,
                &context(frame.depth),
                stats,
                io::ErrorKind::PermissionDenied.into(),
            ));
        }
        self.finished.push(frame.depth);
        Ok(())
    }

    /// Preserves exact accounting and coordinates from the failure boundary.
    fn error(
        &self,
        stage: LocalCopyDirStage,
        context: &CopyTreeFrameContext,
        stats: &LocalCopyDirStats,
        source: io::Error,
    ) -> LocalCopyDirError {
        LocalCopyDirError::new(
            stage,
            context.source.clone(),
            context.destination.clone(),
            *stats,
            source,
        )
    }
}

/// Constructs one deterministic context without consulting a filesystem.
fn context(depth: usize) -> CopyTreeFrameContext {
    CopyTreeFrameContext {
        source: PathBuf::from(format!("source-{depth}")),
        destination: PathBuf::from(format!("target-{depth}")),
        depth,
    }
}

/// Completion is post-order and two descendants consume exactly two entries.
#[test]
fn test_copy_tree_post_order_and_exact_descendant_budget() {
    let live = Rc::new(Cell::new(0));
    let mut backend = Backend {
        live: Rc::clone(&live),
        finished: Vec::new(),
        failure: None,
    };
    let mut budget = CopyBudget::new(
        LocalCopyDirOptions::default()
            .with_max_entries(2)
            .with_max_depth(2)
            .with_max_open_directories(3),
    );
    let frame = Frame::new(0, Rc::clone(&live), &budget);
    let mut stats = LocalCopyDirStats::default();
    copy_tree(&mut backend, frame, &mut stats, &mut budget).expect("exact budget should permit both descendants");
    assert_eq!([2, 1, 0], backend.finished.as_slice());
    assert_eq!(2, stats.directories);
    assert_eq!(0, live.get());
    assert!(
        budget.charge_entry().is_err(),
        "descendants must consume the complete two-entry budget"
    );
}

/// Errors at every backend boundary drop ancestors and return all permits.
#[test]
fn test_copy_tree_failures_release_frames_and_preserve_effects() {
    for stage in [
        LocalCopyDirStage::ReadSourceDirectory,
        LocalCopyDirStage::PrepareDestination,
        LocalCopyDirStage::PreservePermissions,
    ] {
        let live = Rc::new(Cell::new(0));
        let mut backend = Backend {
            live: Rc::clone(&live),
            finished: Vec::new(),
            failure: Some(stage),
        };
        let mut budget = CopyBudget::new(LocalCopyDirOptions::default().with_max_open_directories(3));
        let frame = Frame::new(0, Rc::clone(&live), &budget);
        let mut stats = LocalCopyDirStats::default();
        let error = copy_tree(&mut backend, frame, &mut stats, &mut budget).expect_err("selected boundary must fail");
        assert_eq!(stage, error.stage());
        assert_eq!(&stats, error.stats(), "failure must preserve already-recorded effects");
        assert_eq!(
            if stage == LocalCopyDirStage::ReadSourceDirectory {
                1
            } else {
                2
            },
            stats.directories
        );
        assert_eq!(
            0,
            live.get(),
            "failure must release every ancestor without finish callbacks"
        );
        assert!(backend.finished.is_empty());
        let permits = (0..3)
            .map(|_| budget.acquire_directory().expect("all capacity should have returned"))
            .collect::<Vec<_>>();
        assert!(budget.acquire_directory().is_err());
        drop(permits);
    }
}

/// Rejected depth is checked before publishing or opening the child.
#[test]
fn test_copy_tree_depth_rejection_precedes_child_processing() {
    let live = Rc::new(Cell::new(0));
    let mut backend = Backend {
        live: Rc::clone(&live),
        finished: Vec::new(),
        failure: None,
    };
    let mut budget = CopyBudget::new(LocalCopyDirOptions::default().with_max_depth(1));
    let frame = Frame::new(0, Rc::clone(&live), &budget);
    let mut stats = LocalCopyDirStats::default();
    let error = copy_tree(&mut backend, frame, &mut stats, &mut budget).expect_err("second descendant exceeds depth");
    assert_eq!(LocalCopyDirStage::InspectSourceEntry, error.stage());
    assert_eq!(PathBuf::from("source-2"), error.source_path());
    assert_eq!(1, stats.directories);
    assert_eq!(0, live.get());
}
