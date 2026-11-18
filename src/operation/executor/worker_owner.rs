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
    pub(super) workers: Mutex<Vec<JoinHandle<()>>>,
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
        let current = std::thread::current().id();
        for worker in workers.drain(..) {
            // A queued job can own the last executor handle. In that case the
            // owner is dropped as the job returns on this worker, so joining
            // this handle would make the worker wait for itself. The queue is
            // already stopped; dropping this handle detaches it and lets the
            // worker leave its run loop after the current job finishes.
            if worker.thread().id() == current {
                continue;
            }
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::WorkerOwner;
    use crate::operation::internal::worker::WorkerQueue;

    #[test]
    fn test_drop_stops_queue_and_joins_workers_after_drain() {
        let queue = Arc::new(WorkerQueue::default());
        let worker_queue = Arc::clone(&queue);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (drained_tx, drained_rx) = mpsc::channel();
        let (exited_tx, exited_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            worker_queue.run();
            exited_tx.send(()).unwrap();
        });
        let owner = WorkerOwner::new(Arc::clone(&queue), vec![worker]);

        assert!(
            queue
                .push(
                    Box::new(move || {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }),
                    false,
                    2,
                )
                .is_ok()
        );
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            queue
                .push(Box::new(move || drained_tx.send(()).unwrap()), false, 2)
                .is_ok()
        );

        let (dropped_tx, dropped_rx) = mpsc::channel();
        let drop_thread = std::thread::spawn(move || {
            drop(owner);
            dropped_tx.send(()).unwrap();
        });
        release_tx.send(()).unwrap();
        drained_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        exited_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        dropped_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop_thread.join().unwrap();
        assert!(queue.push(Box::new(|| {}), false, 1).is_err());
    }

    #[test]
    fn test_drop_on_worker_detaches_self_and_exits() {
        let queue = Arc::new(WorkerQueue::default());
        let worker_queue = Arc::clone(&queue);
        let (exited_tx, exited_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            worker_queue.run();
            exited_tx.send(()).unwrap();
        });
        let owner = Arc::new(WorkerOwner::new(Arc::clone(&queue), vec![worker]));
        let worker_owner = Arc::clone(&owner);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (dropped_tx, dropped_rx) = mpsc::channel();
        assert!(
            queue
                .push(
                    Box::new(move || {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        drop(worker_owner);
                        dropped_tx.send(()).unwrap();
                    }),
                    false,
                    1,
                )
                .is_ok()
        );
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(owner);
        release_tx.send(()).unwrap();

        dropped_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        exited_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(queue.push(Box::new(|| {}), false, 1).is_err());
    }
}
