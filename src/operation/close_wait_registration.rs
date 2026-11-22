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
use super::worker_walk_inner::drop_close_waiter;

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
        drop_close_waiter(unused_waker);
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
        drop_close_waiter(removed_waker);
    }
}

#[cfg(test)]
mod tests {
    use std::panic::catch_unwind;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Poll;
    use std::task::Waker;

    use super::CloseWaitRegistration;
    use super::WorkerWalkCloseState;

    struct PanicDropProbe {
        drops: AtomicUsize,
        panic_on_drop: std::sync::atomic::AtomicBool,
    }

    unsafe fn panic_drop_clone(data: *const ()) -> std::task::RawWaker {
        let probe =
            std::mem::ManuallyDrop::new(unsafe { Arc::<PanicDropProbe>::from_raw(data.cast()) });
        std::task::RawWaker::new(Arc::into_raw(Arc::clone(&probe)).cast(), &PANIC_DROP_VTABLE)
    }

    unsafe fn panic_drop_wake(data: *const ()) {
        unsafe { drop(Arc::<PanicDropProbe>::from_raw(data.cast())) };
    }

    unsafe fn panic_drop_wake_by_ref(data: *const ()) {
        let _probe =
            std::mem::ManuallyDrop::new(unsafe { Arc::<PanicDropProbe>::from_raw(data.cast()) });
    }

    unsafe fn panic_drop_drop(data: *const ()) {
        let probe = unsafe { Arc::<PanicDropProbe>::from_raw(data.cast()) };
        probe.drops.fetch_add(1, Ordering::SeqCst);
        if probe.panic_on_drop.swap(false, Ordering::SeqCst) {
            panic!("injected close waiter drop panic");
        }
    }

    static PANIC_DROP_VTABLE: std::task::RawWakerVTable = std::task::RawWakerVTable::new(
        panic_drop_clone,
        panic_drop_wake,
        panic_drop_wake_by_ref,
        panic_drop_drop,
    );

    fn panic_drop_waker(probe: Arc<PanicDropProbe>) -> Waker {
        // SAFETY: the vtable keeps one Arc reference alive per raw waker.
        unsafe {
            Waker::from_raw(std::task::RawWaker::new(
                Arc::into_raw(probe).cast(),
                &PANIC_DROP_VTABLE,
            ))
        }
    }

    fn panic_drop_probe() -> Arc<PanicDropProbe> {
        Arc::new(PanicDropProbe {
            drops: AtomicUsize::new(0),
            panic_on_drop: std::sync::atomic::AtomicBool::new(true),
        })
    }

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

    #[test]
    #[cfg(panic = "unwind")]
    fn test_terminal_registration_survives_unused_waker_drop_panic() {
        for (result, is_error) in [
            (Ok(()), false),
            (
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "original close failure",
                )),
                true,
            ),
        ] {
            let state = Arc::new(Mutex::new(WorkerWalkCloseState::default()));
            state.lock().unwrap().publish(result);
            let probe = panic_drop_probe();
            let waker = panic_drop_waker(Arc::clone(&probe));
            let mut registration = CloseWaitRegistration::new(Arc::clone(&state));

            let polled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                registration.poll(&waker)
            }));
            assert!(
                polled.is_ok(),
                "unused waker destructor must not replace terminal result"
            );
            match polled.unwrap() {
                Poll::Ready(Ok(())) if !is_error => {}
                Poll::Ready(Err(error)) if is_error => {
                    assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
                    assert_eq!("original close failure", error.to_string());
                }
                result => panic!("unexpected terminal registration result: {result:?}"),
            }
            assert_eq!(1, probe.drops.load(Ordering::SeqCst));
            drop(registration);
            drop(waker);
        }
    }

    #[test]
    #[cfg(panic = "unwind")]
    fn test_replacement_and_cancellation_survive_waker_drop_panic() {
        let state = Arc::new(Mutex::new(WorkerWalkCloseState::default()));
        let mut canceled = CloseWaitRegistration::new(Arc::clone(&state));
        let canceled_probe = panic_drop_probe();
        let canceled_waker = panic_drop_waker(Arc::clone(&canceled_probe));
        assert!(canceled.poll(&canceled_waker).is_pending());
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(canceled_waker)))
                .is_err()
        );
        canceled_probe.panic_on_drop.store(true, Ordering::SeqCst);

        let replacement_probe = panic_drop_probe();
        let replacement_waker = panic_drop_waker(Arc::clone(&replacement_probe));
        let replaced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            canceled.poll(&replacement_waker)
        }));
        assert!(replaced.is_ok());
        assert!(replaced.unwrap().is_pending());
        assert_eq!(
            2,
            canceled_probe.drops.load(Ordering::SeqCst),
            "replaced registration waker was disposed"
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(replacement_waker)))
                .is_err()
        );
        replacement_probe
            .panic_on_drop
            .store(true, Ordering::SeqCst);

        let mut other = CloseWaitRegistration::new(Arc::clone(&state));
        assert!(other.poll(Waker::noop()).is_pending());
        let canceled_drop =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(canceled)));
        assert!(
            canceled_drop.is_ok(),
            "cancellation must contain the removed waker destructor panic"
        );
        assert_eq!(2, replacement_probe.drops.load(Ordering::SeqCst));
        assert_eq!(
            1,
            state.lock().unwrap().waiters.len(),
            "only the canceled registration is removed"
        );
        state.lock().unwrap().publish(Ok(()));
        assert!(
            other.poll(Waker::noop()).is_ready(),
            "other registration still observes completion"
        );
    }
}
