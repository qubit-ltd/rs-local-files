// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded ordinary and cleanup queues used by local I/O workers.

use std::collections::VecDeque;
use std::sync::Condvar;
use std::sync::Mutex;

use super::super::permits::Permit;
use super::CleanupCompletionEnqueueError;
use super::CleanupEnqueueError;
use crate::operation::stream::dispose_panic_payload;
// Stores bounded queue contents and the worker's fair scheduling state.
mod queue_state;

use queue_state::QueueState;

/// Runs queued local I/O jobs while preserving a cleanup execution lane.
pub(crate) struct WorkerQueue {
    /// Queue state shared with producers and the worker thread.
    state: Mutex<QueueState>,
    /// Wakes the worker when work is enqueued or shutdown is requested.
    wake: Condvar,
}

/// Creates an empty queue with its ordinary and cleanup lanes ready.
impl Default for WorkerQueue {
    /// Returns an empty queue with both scheduling lanes initialized.
    fn default() -> Self {
        Self {
            state: Mutex::new(QueueState {
                ordinary: VecDeque::new(),
                cleanup: VecDeque::new(),
                stopping: false,
                last_was_cleanup: false,
            }),
            wake: Condvar::new(),
        }
    }
}

impl WorkerQueue {
    /// Returns the number of ordinary jobs waiting for a worker.
    #[cfg(test)]
    pub(crate) fn ordinary_len(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).ordinary.len()
    }

    /// Enqueues ordinary or cleanup work up to the selected queue limit.
    ///
    /// # Parameters
    ///
    /// - `job`: Owned task to run on the worker thread.
    /// - `cleanup`: Selects the cleanup queue when true.
    /// - `limit`: Maximum number of tasks allowed in the selected queue.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` after enqueueing or the original task when the queue is
    /// stopped or full.
    pub(crate) fn push(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        cleanup: bool,
        limit: usize,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping {
            return Err(job);
        }
        let queue = if cleanup {
            &mut state.cleanup
        } else {
            &mut state.ordinary
        };
        if queue.len() >= limit {
            return Err(job);
        }
        queue.push_back(job);
        self.wake.notify_one();
        Ok(())
    }

    /// Enqueues cleanup work using its pre-reserved cleanup permit.
    ///
    /// # Parameters
    ///
    /// - `job`: Owned cleanup task to run on the worker thread.
    /// - `permit`: Capacity reserved for this cleanup operation.
    /// - `limit`: Maximum number of cleanup tasks allowed in the queue.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` after enqueueing or both the task and permit when the
    /// queue is stopped or full.
    pub(crate) fn push_cleanup(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
        limit: usize,
    ) -> Result<(), CleanupEnqueueError> {
        self.push_cleanup_with_completion(job, permit, Box::new(|| {}), limit)
            .map_err(|(job, permit, _completion)| (job, permit))
    }

    /// Enqueues cleanup work and runs `completion` only after its permit has
    /// been returned to the cleanup pool.
    ///
    /// # Parameters
    /// - `job` — cleanup task to execute.
    /// - `permit` — capacity reserved for that cleanup task.
    /// - `completion` — callback invoked after the permit is released.
    /// - `limit` — maximum number of cleanup tasks allowed in the queue.
    pub(crate) fn push_cleanup_with_completion(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
        completion: Box<dyn FnOnce() + Send + 'static>,
        limit: usize,
    ) -> Result<(), CleanupCompletionEnqueueError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping || state.cleanup.len() >= limit {
            return Err((job, permit, completion));
        }
        let wrapped = Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            drop(permit);
            match result {
                Ok(()) => completion(),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        });
        state.cleanup.push_back(wrapped);
        self.wake.notify_one();
        Ok(())
    }

    /// Runs queued tasks until shutdown has been requested and both queues
    /// drain.
    pub(crate) fn run(&self) {
        loop {
            let job = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                while state.ordinary.is_empty() && state.cleanup.is_empty() && !state.stopping {
                    state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                if state.stopping && state.ordinary.is_empty() && state.cleanup.is_empty() {
                    return;
                }
                let choose_cleanup = if state.cleanup.is_empty() {
                    false
                } else if state.ordinary.is_empty() {
                    true
                } else {
                    !state.last_was_cleanup
                };
                state.last_was_cleanup = choose_cleanup;
                if choose_cleanup {
                    state.cleanup.pop_front()
                } else {
                    state.ordinary.pop_front()
                }
            };
            if let Some(job) = job
                && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
            {
                dispose_panic_payload(payload);
            }
        }
    }

    /// Requests worker shutdown after queued work has been processed.
    pub(crate) fn stop(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopping = true;
        self.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::WorkerQueue;
    use crate::operation::internal::permits::PermitPool;

    #[test]
    fn test_stop_drains_enqueued_ordinary_and_cleanup_jobs() {
        let queue = WorkerQueue::default();
        let (tx, rx) = mpsc::channel();
        let ordinary_tx = tx.clone();
        assert!(
            queue
                .push(Box::new(move || ordinary_tx.send("ordinary").unwrap()), false, 1)
                .is_ok()
        );

        let permits = PermitPool::new(1);
        let permit = permits.try_acquire(1).unwrap();
        let cleanup_tx = tx.clone();
        assert!(
            queue
                .push_cleanup(Box::new(move || cleanup_tx.send("cleanup").unwrap()), permit, 1,)
                .is_ok()
        );
        drop(tx);

        queue.stop();
        queue.run();

        let mut completed = vec![
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ];
        completed.sort_unstable();
        assert_eq!(completed, ["cleanup", "ordinary"]);
        assert_eq!(permits.used(), 0);
    }

    #[test]
    fn test_stopped_queue_returns_rejected_jobs_and_cleanup_permits() {
        let queue = WorkerQueue::default();
        queue.stop();

        let (ordinary_tx, ordinary_rx) = mpsc::channel();
        let ordinary_job = Box::new(move || ordinary_tx.send(()).unwrap());
        let ordinary_job = queue.push(ordinary_job, false, 1).unwrap_err();
        ordinary_job();
        ordinary_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let permits = PermitPool::new(1);
        let permit = permits.try_acquire(1).unwrap();
        let (cleanup_tx, cleanup_rx) = mpsc::channel();
        let cleanup_job = Box::new(move || cleanup_tx.send(()).unwrap());
        let (cleanup_job, permit) = queue.push_cleanup(cleanup_job, permit, 1).unwrap_err();
        assert_eq!(permits.used(), 1);
        cleanup_job();
        drop(permit);
        cleanup_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(permits.used(), 0);
    }

    #[test]
    fn test_cleanup_completion_runs_after_its_permit_is_returned() {
        let queue = WorkerQueue::default();
        let permits = PermitPool::new(1);
        let permit = permits.try_acquire(1).unwrap();
        let (tx, rx) = mpsc::channel();
        let completion = Box::new(move || tx.send(()).unwrap());
        assert!(
            queue
                .push_cleanup_with_completion(Box::new(|| {}), permit, completion, 1)
                .is_ok()
        );
        queue.stop();
        queue.run();
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(permits.used(), 0);
    }

    #[test]
    fn test_cleanup_completion_does_not_confirm_a_panicking_job() {
        let queue = WorkerQueue::default();
        let permits = PermitPool::new(1);
        let permit = permits.try_acquire(1).unwrap();
        let (tx, rx) = mpsc::channel();
        let completion = Box::new(move || tx.send(()).unwrap());
        assert!(
            queue
                .push_cleanup_with_completion(Box::new(|| panic!("cleanup did not finish")), permit, completion, 1,)
                .is_ok()
        );
        queue.stop();
        queue.run();
        assert!(rx.try_recv().is_err());
        assert_eq!(permits.used(), 0);
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_worker_continues_after_panicking_task_payload_drop() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("task panic payload destructor panicked");
            }
        }

        let queue = std::sync::Arc::new(WorkerQueue::default());
        let panicking_queue = std::sync::Arc::clone(&queue);
        assert!(
            panicking_queue
                .push(Box::new(|| std::panic::panic_any(PanicOnDrop)), false, 2)
                .is_ok()
        );
        let (completed_tx, completed_rx) = mpsc::channel();
        assert!(
            queue
                .push(Box::new(move || completed_tx.send(()).unwrap()), false, 2)
                .is_ok()
        );

        let worker_queue = std::sync::Arc::clone(&queue);
        let worker = std::thread::spawn(move || worker_queue.run());
        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the worker completes the task after the panicking task");
        queue.stop();
        worker.join().expect("the worker remains alive to drain work");
    }

    #[test]
    fn test_rejected_cleanup_completion_preserves_job_and_permit() {
        let queue = WorkerQueue::default();
        queue.stop();
        let permits = PermitPool::new(1);
        let permit = permits.try_acquire(1).unwrap();
        let (tx, rx) = mpsc::channel();
        let completion = Box::new(move || tx.send("completion").unwrap());
        let (job, permit, completion) = queue
            .push_cleanup_with_completion(Box::new(|| {}), permit, completion, 1)
            .unwrap_err();
        assert_eq!(permits.used(), 1);
        job();
        drop(permit);
        completion();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "completion");
        assert_eq!(permits.used(), 0);
    }

    #[test]
    fn test_run_alternates_lanes_while_both_have_pending_jobs() {
        let queue = WorkerQueue::default();
        let (tx, rx) = mpsc::channel();

        for label in ["ordinary-1", "ordinary-2"] {
            let tx = tx.clone();
            assert!(queue.push(Box::new(move || tx.send(label).unwrap()), false, 2).is_ok());
        }

        let permits = PermitPool::new(2);
        for label in ["cleanup-1", "cleanup-2"] {
            let permit = permits.try_acquire(1).unwrap();
            let tx = tx.clone();
            assert!(
                queue
                    .push_cleanup(Box::new(move || tx.send(label).unwrap()), permit, 2)
                    .is_ok()
            );
        }

        drop(tx);
        queue.stop();
        queue.run();

        let completed = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(1)).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(completed, ["cleanup-1", "ordinary-1", "cleanup-2", "ordinary-2"]);
        assert_eq!(permits.used(), 0);
    }

    #[test]
    fn test_queue_recovers_and_drains_after_state_mutex_is_poisoned() {
        use std::panic::AssertUnwindSafe;
        use std::panic::catch_unwind;

        let queue = WorkerQueue::default();
        let poison = catch_unwind(AssertUnwindSafe(|| {
            let _state = queue.state.lock().unwrap();
            panic!("poison worker queue state");
        }));
        assert!(poison.is_err());

        let (tx, rx) = mpsc::channel();
        let ordinary_tx = tx.clone();
        assert!(
            queue
                .push(Box::new(move || ordinary_tx.send("ordinary").unwrap()), false, 1)
                .is_ok()
        );

        let permits = PermitPool::new(1);
        let permit = permits.try_acquire(1).unwrap();
        let cleanup_tx = tx.clone();
        assert!(
            queue
                .push_cleanup(Box::new(move || cleanup_tx.send("cleanup").unwrap()), permit, 1)
                .is_ok()
        );
        drop(tx);

        queue.stop();
        queue.run();

        let mut completed = vec![
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ];
        completed.sort_unstable();
        assert_eq!(completed, ["cleanup", "ordinary"]);
        assert_eq!(permits.used(), 0);
    }
}
