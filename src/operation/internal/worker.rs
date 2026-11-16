// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded ordinary and cleanup queues used by local I/O workers.

use std::sync::Condvar;
use std::sync::Mutex;

use super::permits::Permit;
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

impl Default for WorkerQueue {
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
    ) -> Result<(), (Box<dyn FnOnce() + Send + 'static>, Permit)> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping || state.cleanup.len() >= limit {
            return Err((job, permit));
        }
        let wrapped = Box::new(move || {
            let _permit = permit;
            job();
        });
        state.cleanup.push_back(wrapped);
        self.wake.notify_one();
        Ok(())
    }

    /// Runs queued tasks until shutdown has been requested and both queues drain.
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
            if let Some(job) = job {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            }
        }
    }

    /// Requests worker shutdown after queued work has been processed.
    pub(crate) fn stop(&self) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopping = true;
        self.wake.notify_all();
    }
}
