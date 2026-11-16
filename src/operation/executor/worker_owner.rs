// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;

use super::super::internal::worker::WorkerQueue;

/// Represents worker owner within the local operation subsystem.
pub(super) struct WorkerOwner {
    /// Stores the queue associated with this value.
    queue: Arc<WorkerQueue>,
    /// Stores the workers associated with this value.
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl WorkerOwner {
    /// Retains the queue and joins its worker threads when the owner is
    /// dropped.
    ///
    /// # Parameters
    /// - `queue` — worker queue stopped when this owner is dropped.
    /// - `workers` — threads joined when this owner is dropped.
    pub(super) fn new(queue: Arc<WorkerQueue>, workers: Vec<JoinHandle<()>>) -> Self {
        Self {
            queue,
            workers: Mutex::new(workers),
        }
    }
}

impl Drop for WorkerOwner {
    /// Runs the drop behavior while preserving the operation lifecycle
    /// invariants.
    fn drop(&mut self) {
        self.queue.stop();
        let workers = self.workers.get_mut().unwrap_or_else(|e| e.into_inner());
        for worker in workers.drain(..) {
            let _ = worker.join();
        }
    }
}
