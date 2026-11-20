// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::task::Poll;
use std::task::Waker;

use super::worker_walk_command::WorkerWalkCommand;
use super::worker_walk_cursor::WorkerWalkCursor;

/// Shared executor, iterator, and close state for one directory worker.
pub(super) enum WorkerWalkCloseState {
    /// Close has not completed; all active waiters are registered here.
    Waiting(Vec<Waker>),
    /// Iterator cleanup and reserved capacity release completed.
    Closed,
    /// Close could not be confirmed; retain a repeatable I/O error.
    Failed(io::ErrorKind, String),
}

pub(super) struct WorkerWalkInner {
    /// Executor that serializes directory operations and cleanup.
    pub(super) executor: super::super::LocalIoExecutor,
    /// Native iterator and buffered entry guarded as one worker state.
    pub(super) walker: Mutex<WorkerWalkCursor>,
    /// Results keyed by stable caller request identifiers.
    pub(super) commands: Mutex<HashMap<u64, WorkerWalkCommand>>,
    /// Signals that no new directory entries should be read.
    pub(super) stop_requested: AtomicBool,
    /// Close result and all tasks waiting for its publication.
    pub(super) close_state: Mutex<WorkerWalkCloseState>,
    /// Reserved capacity for worker-side cleanup.
    pub(super) cleanup_permit: Mutex<Option<super::super::internal::permits::Permit>>,
    /// Budget retained while returned path batches remain alive.
    pub(super) _buffer_budget: Arc<Mutex<super::super::internal::permits::Permit>>,
    /// Budget retained while the native directory handle is open.
    pub(super) _handle_budget: Mutex<Option<Arc<super::super::internal::permits::Permit>>>,
}

impl WorkerWalkInner {
    /// Registers a waiter atomically with checking the published close result.
    pub(super) fn poll_closed(&self, waker: &Waker) -> Poll<io::Result<()>> {
        let mut state = self.close_state.lock().unwrap_or_else(|e| e.into_inner());
        match &mut *state {
            WorkerWalkCloseState::Waiting(waiters) => {
                if !waiters.iter().any(|registered| registered.will_wake(waker)) {
                    waiters.push(waker.clone());
                }
                Poll::Pending
            }
            WorkerWalkCloseState::Closed => Poll::Ready(Ok(())),
            WorkerWalkCloseState::Failed(kind, message) => {
                Poll::Ready(Err(io::Error::new(*kind, message.clone())))
            }
        }
    }

    /// Publishes one terminal result, then notifies every waiter outside the lock.
    pub(super) fn publish_close(&self, result: io::Result<()>) {
        let waiters = {
            let mut state = self.close_state.lock().unwrap_or_else(|e| e.into_inner());
            let WorkerWalkCloseState::Waiting(waiters) = &mut *state else {
                return;
            };
            let waiters = std::mem::take(waiters);
            *state = match result {
                Ok(()) => WorkerWalkCloseState::Closed,
                Err(error) => WorkerWalkCloseState::Failed(error.kind(), error.to_string()),
            };
            waiters
        };
        for waker in waiters {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
        }
    }

    /// Releases the native handle reservation exactly once after iterator
    /// close.
    pub(super) fn release_handle_budget(&self) {
        self._handle_budget.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}
