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
        let registered_waker = waker.clone();
        let (result, unused_waker) = {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .poll(self.id, registered_waker)
        };
        drop(unused_waker);
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

#[cfg(test)]
mod tests {
    use std::panic::catch_unwind;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Poll;
    use std::task::Waker;

    use super::CloseWaitRegistration;
    use super::WorkerWalkCloseState;

    #[test]
    fn test_close_registration_recovers_poisoned_state_for_new_poll_and_drop() {
        let state = Arc::new(Mutex::new(WorkerWalkCloseState::default()));
        let poisoned = Arc::clone(&state);
        let _ = catch_unwind(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("poison the close state for recovery coverage");
        });

        let mut registration = CloseWaitRegistration::new(Arc::clone(&state));
        assert!(matches!(registration.poll(Waker::noop()), Poll::Pending));
        let waiter_count = match state.lock() {
            Ok(_) => panic!("the state should remain poisoned"),
            Err(poisoned) => poisoned.into_inner().waiters.len(),
        };
        assert_eq!(1, waiter_count);

        drop(registration);
        let waiters_empty = match state.lock() {
            Ok(_) => panic!("the state should remain poisoned"),
            Err(poisoned) => poisoned.into_inner().waiters.is_empty(),
        };
        assert!(waiters_empty);
    }
}
