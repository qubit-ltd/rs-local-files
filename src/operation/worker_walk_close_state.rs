// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::io;
use std::task::Poll;
use std::task::Waker;

/// Shared terminal result and waiters for a worker-backed directory walk.
#[derive(Default)]
pub(super) struct WorkerWalkCloseState {
    /// Published close result; `None` means cleanup is still pending.
    terminal: Option<Result<(), (io::ErrorKind, String)>>,
    /// Independent tasks waiting for terminal close publication.
    pub(super) waiters: HashMap<u64, Waker>,
    /// Compatibility waiters used by the worker driver's internal interface.
    legacy_waiters: Vec<Waker>,
    /// Next unique identifier assigned to a cancellable close waiter.
    pub(super) next_waiter_id: u64,
}

impl WorkerWalkCloseState {
    /// Returns the close result or registers a compatibility waiter.
    ///
    /// # Parameters
    /// - `waker` — task to notify when close reaches a terminal state.
    pub(super) fn poll_legacy(&mut self, waker: Waker) -> (Poll<io::Result<()>>, Option<Waker>) {
        if let Some(result) = &self.terminal {
            let result = match result {
                Ok(()) => Poll::Ready(Ok(())),
                Err((kind, message)) => Poll::Ready(Err(io::Error::new(*kind, message.clone()))),
            };
            return (result, Some(waker));
        }
        if self
            .legacy_waiters
            .iter()
            .any(|registered| registered.will_wake(&waker))
        {
            return (Poll::Pending, Some(waker));
        }
        self.legacy_waiters.push(waker);
        (Poll::Pending, None)
    }

    /// Polls a registered waiter and updates only that waiter's waker.
    ///
    /// # Parameters
    /// - `id` — unique identity of this waiting future.
    /// - `waker` — current task waker.
    pub(super) fn poll(&mut self, id: u64, waker: Waker) -> (Poll<io::Result<()>>, Option<Waker>) {
        if let Some(result) = &self.terminal {
            let result = match result {
                Ok(()) => Poll::Ready(Ok(())),
                Err((kind, message)) => Poll::Ready(Err(io::Error::new(*kind, message.clone()))),
            };
            return (result, Some(waker));
        }
        let replaced = self.waiters.insert(id, waker);
        (Poll::Pending, replaced)
    }

    /// Publishes a terminal result once and returns all registered waiters.
    ///
    /// # Parameters
    /// - `result` — confirmed close or repeatable close failure to publish.
    pub(super) fn publish(&mut self, result: io::Result<()>) -> Option<Vec<Waker>> {
        if self.terminal.is_some() {
            return None;
        }
        self.terminal = Some(match result {
            Ok(()) => Ok(()),
            Err(error) => Err((error.kind(), error.to_string())),
        });
        let mut waiters = std::mem::take(&mut self.waiters)
            .into_values()
            .collect::<Vec<_>>();
        waiters.append(&mut self.legacy_waiters);
        Some(waiters)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use super::super::close_wait_registration::CloseWaitRegistration;
    use super::WorkerWalkCloseState;

    #[derive(Debug)]
    struct TestWake(AtomicUsize);

    impl Wake for TestWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct LockCheckingWake {
        state: Arc<Mutex<WorkerWalkCloseState>>,
        unlocked_drops: Arc<AtomicUsize>,
    }

    impl Wake for LockCheckingWake {
        fn wake(self: Arc<Self>) {
            self.unlocked_drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl Drop for LockCheckingWake {
        fn drop(&mut self) {
            if let Ok(guard) = self.state.try_lock() {
                drop(guard);
                self.unlocked_drops.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[test]
    fn test_canceled_close_registration_releases_only_its_waker() {
        let state = Arc::new(std::sync::Mutex::new(WorkerWalkCloseState::default()));
        let first_wake = Arc::new(TestWake(AtomicUsize::new(0)));
        let first_wake_weak = Arc::downgrade(&first_wake);
        let first_waker = Waker::from(Arc::clone(&first_wake));
        let second_wake = Arc::new(TestWake(AtomicUsize::new(0)));
        let second_wake_weak = Arc::downgrade(&second_wake);
        let second_waker = Waker::from(Arc::clone(&second_wake));
        let mut first = CloseWaitRegistration::new(Arc::clone(&state));
        let mut second = CloseWaitRegistration::new(Arc::clone(&state));
        assert!(first.poll(&first_waker).is_pending());
        assert!(second.poll(&first_waker).is_pending());
        assert!(first.poll(&second_waker).is_pending());
        drop(first_waker);
        drop(first_wake);
        drop(second_waker);
        drop(second_wake);
        assert!(first_wake_weak.upgrade().is_some());
        assert!(second_wake_weak.upgrade().is_some());
        drop(first);
        assert_eq!(state.lock().unwrap().waiters.len(), 1);
        assert!(first_wake_weak.upgrade().is_some());
        assert!(second_wake_weak.upgrade().is_none());
        drop(second);
        assert!(first_wake_weak.upgrade().is_none());
    }

    #[test]
    fn test_close_registration_recovers_from_poisoned_shared_state() {
        let state = Arc::new(Mutex::new(WorkerWalkCloseState::default()));
        let poisoned_state = Arc::clone(&state);
        let _ = std::panic::catch_unwind(move || {
            let _guard = poisoned_state.lock().unwrap();
            panic!("poison close state for recovery test");
        });

        let mut registration = CloseWaitRegistration::new(Arc::clone(&state));
        assert!(registration.poll(Waker::noop()).is_pending());
        drop(registration);

        let state = state.lock().unwrap_or_else(|error| error.into_inner());
        assert!(state.waiters.is_empty());
    }

    #[test]
    fn test_replaced_and_canceled_wakers_are_dropped_outside_close_lock() {
        let state = Arc::new(Mutex::new(WorkerWalkCloseState::default()));
        let unlocked_drops = Arc::new(AtomicUsize::new(0));
        let mut registration = CloseWaitRegistration::new(Arc::clone(&state));

        let replaced = Arc::new(LockCheckingWake {
            state: Arc::clone(&state),
            unlocked_drops: Arc::clone(&unlocked_drops),
        });
        let replaced_waker = Waker::from(Arc::clone(&replaced));
        assert!(registration.poll(&replaced_waker).is_pending());
        drop(replaced_waker);
        drop(replaced);
        assert!(registration.poll(Waker::noop()).is_pending());
        assert_eq!(unlocked_drops.load(Ordering::Relaxed), 1);

        let canceled = Arc::new(LockCheckingWake {
            state: Arc::clone(&state),
            unlocked_drops: Arc::clone(&unlocked_drops),
        });
        let canceled_waker = Waker::from(Arc::clone(&canceled));
        assert!(registration.poll(&canceled_waker).is_pending());
        drop(canceled_waker);
        drop(canceled);
        drop(registration);
        assert_eq!(unlocked_drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_terminal_poll_drops_unused_waker_outside_close_lock() {
        let state = Arc::new(Mutex::new(WorkerWalkCloseState::default()));
        state.lock().unwrap().publish(Ok(()));
        let unlocked_drops = Arc::new(AtomicUsize::new(0));
        let terminal_wake = Arc::new(LockCheckingWake {
            state: Arc::clone(&state),
            unlocked_drops: Arc::clone(&unlocked_drops),
        });
        let terminal_waker = Waker::from(Arc::clone(&terminal_wake));
        let mut registration = CloseWaitRegistration::new(Arc::clone(&state));

        assert!(registration.poll(&terminal_waker).is_ready());
        drop(terminal_waker);
        drop(terminal_wake);
        assert_eq!(unlocked_drops.load(Ordering::Relaxed), 1);
    }
}
