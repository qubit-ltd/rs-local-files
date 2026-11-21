// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Poll;
use std::task::Waker;

use super::worker_walk_close_state::WorkerWalkCloseState;

/// One cancellable registration in the native walk close state.
pub(super) struct CloseWaitRegistration {
    /// Shared terminal state and waiter registry.
    state: Arc<Mutex<WorkerWalkCloseState>>,
    /// Unique identifier removed when this registration is dropped.
    id: u64,
}

impl CloseWaitRegistration {
    /// Creates a unique registration without adding a waker until first poll.
    ///
    /// # Parameters
    /// - `state` — close result and waiter registry shared with the worker.
    pub(super) fn new(state: Arc<Mutex<WorkerWalkCloseState>>) -> Self {
        let mut close_state = state.lock().unwrap_or_else(|error| error.into_inner());
        close_state.next_waiter_id = close_state.next_waiter_id.wrapping_add(1).max(1);
        let id = close_state.next_waiter_id;
        drop(close_state);
        Self { state, id }
    }

    /// Polls the shared terminal result and refreshes this waiter's waker.
    ///
    /// # Parameters
    /// - `waker` — current task waker to notify when close is published.
    pub(super) fn poll(&mut self, waker: &Waker) -> Poll<io::Result<()>> {
        let (result, replaced_waker) = {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .poll(self.id, waker)
        };
        drop(replaced_waker);
        result
    }
}

impl Drop for CloseWaitRegistration {
    /// Removes this waiter's waker when its future completes or is cancelled.
    fn drop(&mut self) {
        let removed_waker = {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .waiters
                .remove(&self.id)
        };
        drop(removed_waker);
    }
}
