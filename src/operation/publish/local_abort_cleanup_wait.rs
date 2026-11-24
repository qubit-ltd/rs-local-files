// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use qubit_operation::CleanupOutcome;

use super::LocalAbortCleanupState;
use super::LocalManagedAbortResource;

/// Future that returns the worker-confirmed abort cleanup outcome.
///
/// # Type Parameters
///
/// * `W` - Native writer type owned by the pending cleanup.
pub(in crate::operation) struct LocalAbortCleanupWait<W: Send + 'static>(
    /// Shared state used to observe worker completion and register the waker.
    pub(in crate::operation) Arc<LocalAbortCleanupState<W>>,
);

impl<W: Send + 'static> Future for LocalAbortCleanupWait<W> {
    /// Cleanup outcome returned when the worker finishes aborting the writer.
    type Output = CleanupOutcome<LocalManagedAbortResource<W>>;

    /// Polls for the worker result and registers the current task for wakeup.
    ///
    /// # Parameters
    ///
    /// * `self` - Pending cleanup wait future.
    /// * `context` - Task context used to install the completion waker.
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let result = { self.0.result.lock().unwrap_or_else(|error| error.into_inner()).take() };
        if let Some(result) = result {
            let old_waker = self.0.waker.lock().unwrap_or_else(|error| error.into_inner()).take();
            super::super::temp::safe_drop_waker(old_waker);
            return Poll::Ready(result);
        }
        let Some(new_waker) = super::super::temp::safe_clone_waker(context.waker()) else {
            // The worker may have published while cloning the hostile waker.
            // Recheck before returning Pending, and release this waiter's old
            // registration when no further notification is needed.
            let result = { self.0.result.lock().unwrap_or_else(|error| error.into_inner()).take() };
            if let Some(result) = result {
                let old_waker = self.0.waker.lock().unwrap_or_else(|error| error.into_inner()).take();
                super::super::temp::safe_drop_waker(old_waker);
                return Poll::Ready(result);
            }
            return Poll::Pending;
        };
        let old_waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(new_waker);
        super::super::temp::safe_drop_waker(old_waker);
        let result = { self.0.result.lock().unwrap_or_else(|error| error.into_inner()).take() };
        match result {
            Some(result) => {
                let old_waker = self.0.waker.lock().unwrap_or_else(|error| error.into_inner()).take();
                super::super::temp::safe_drop_waker(old_waker);
                Poll::Ready(result)
            }
            None => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Waker;

    use qubit_operation::CleanupOutcome;

    use super::super::LocalAbortCleanupState;
    use super::LocalAbortCleanupWait;

    struct PublishOnCloneProbe {
        state: Arc<LocalAbortCleanupState<u8>>,
        drops: AtomicUsize,
        locks_available: AtomicBool,
        panic_on_drop: bool,
    }

    unsafe fn publish_abort_during_clone(data: *const ()) -> std::task::RawWaker {
        // SAFETY: the probe remains alive through the poll and registered waker drop.
        let probe = unsafe { &*(data.cast::<PublishOnCloneProbe>()) };
        *probe.state.result.lock().unwrap() = Some(CleanupOutcome::Released);
        std::task::RawWaker::new(data, &PUBLISH_ON_CLONE_VTABLE)
    }

    unsafe fn drop_publish_on_clone_waker(data: *const ()) {
        // SAFETY: the probe remains alive until after the poll returns.
        let probe = unsafe { &*(data.cast::<PublishOnCloneProbe>()) };
        probe.drops.fetch_add(1, Ordering::SeqCst);
        let result_available = probe.state.result.try_lock().is_ok();
        let waker_available = probe.state.waker.try_lock().is_ok();
        probe
            .locks_available
            .store(result_available && waker_available, Ordering::SeqCst);
        if probe.panic_on_drop {
            panic!("injected abort waiter waker drop panic");
        }
    }

    static PUBLISH_ON_CLONE_VTABLE: std::task::RawWakerVTable =
        std::task::RawWakerVTable::new(publish_abort_during_clone, |_| {}, |_| {}, drop_publish_on_clone_waker);

    fn wait<W: Send + 'static>() -> (LocalAbortCleanupWait<W>, Arc<LocalAbortCleanupState<W>>) {
        let state = Arc::new(LocalAbortCleanupState {
            owner: Mutex::new(None),
            result: Mutex::new(None),
            pending_result: Mutex::new(None),
            waker: Mutex::new(None),
        });
        (LocalAbortCleanupWait(Arc::clone(&state)), state)
    }

    #[test]
    fn test_abort_wait_returns_a_result_present_before_the_first_poll() {
        let (mut wait, state) = wait::<u8>();
        *state.result.lock().expect("cleanup result lock") = Some(CleanupOutcome::Released);
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
    }

    #[test]
    fn test_abort_wait_observes_worker_result_after_registering_waker() {
        let (mut wait, state) = wait::<u8>();
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Pending
        ));
        assert!(state.waker.lock().expect("registered waker lock").is_some());

        // This is the worker's publication point after a pending poll; the
        // next poll must consume the outcome even if the caller missed a wake.
        *state.result.lock().expect("cleanup result lock") = Some(CleanupOutcome::Released);
        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
    }

    #[test]
    fn test_abort_wait_recovers_a_poisoned_result_lock() {
        let (mut wait, state) = wait::<u8>();
        let result_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            *result_state.result.lock().expect("cleanup result lock") = Some(CleanupOutcome::Released);
            panic!("poison cleanup result lock");
        });
        assert!(poisoner.join().is_err());
        let mut context = Context::from_waker(Waker::noop());

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
    }

    #[test]
    fn test_abort_wait_consumes_ready_result_after_waker_lock_poison() {
        let (mut wait, state) = wait::<u8>();
        *state.result.lock().expect("cleanup result lock") = Some(CleanupOutcome::Released);
        let waker_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _guard = waker_state.waker.lock().expect("registered waker lock");
            panic!("poison registered waker lock after result publication");
        });
        assert!(poisoner.join().is_err());
        let mut context = Context::from_waker(Waker::noop());

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
        assert!(state.waker.lock().unwrap_or_else(|error| error.into_inner()).is_none());
    }

    #[test]
    fn test_abort_wait_registers_waker_after_poisoned_waker_lock() {
        let (mut wait, state) = wait::<u8>();
        let waker_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _guard = waker_state.waker.lock().expect("registered waker lock");
            panic!("poison registered waker lock");
        });
        assert!(poisoner.join().is_err());
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&state.result);
        let wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let waker = Waker::from(Arc::new(TestWake(Arc::clone(&wake_count))));
        let mut context = Context::from_waker(&waker);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Pending
        ));
        let registered_waker = state.waker.lock().unwrap_or_else(|error| error.into_inner());
        let registered_waker = registered_waker.as_ref().expect("pending poll registers a waker");
        registered_waker.wake_by_ref();
        assert_eq!(1, wake_count.load(std::sync::atomic::Ordering::SeqCst));
    }

    struct TestWake(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl std::task::Wake for TestWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn test_abort_wait_replaces_waker_when_a_second_poll_is_pending() {
        let (mut wait, state) = wait::<u16>();
        let first_wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let replacement_wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let first_waker = Waker::from(Arc::new(TestWake(Arc::clone(&first_wake_count))));
        let mut first_context = Context::from_waker(&first_waker);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut first_context),
            Poll::Pending
        ));

        let replacement_waker = Waker::from(Arc::new(TestWake(Arc::clone(&replacement_wake_count))));
        let mut replacement_context = Context::from_waker(&replacement_waker);
        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut replacement_context),
            Poll::Pending
        ));

        let registered_waker = state.waker.lock().expect("registered waker lock");
        registered_waker
            .as_ref()
            .expect("second pending poll retains a waker")
            .wake_by_ref();
        assert_eq!(0, first_wake_count.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(1, replacement_wake_count.load(std::sync::atomic::Ordering::SeqCst));
    }

    fn panic_on_clone(_: *const ()) -> std::task::RawWaker {
        std::panic::panic_any(());
    }

    unsafe fn publish_then_panic_on_clone(data: *const ()) -> std::task::RawWaker {
        // SAFETY: the raw waker owns one Arc reference and this callback
        // temporarily increments it before accessing the shared test state.
        unsafe { Arc::<LocalAbortCleanupState<u8>>::increment_strong_count(data.cast()) };
        let state = unsafe { Arc::<LocalAbortCleanupState<u8>>::from_raw(data.cast()) };
        *state.result.lock().expect("cleanup result lock") = Some(CleanupOutcome::Released);
        std::panic::panic_any(());
    }

    #[cfg(panic = "unwind")]
    unsafe fn publish_then_poison_result_and_panic_on_clone(data: *const ()) -> std::task::RawWaker {
        // SAFETY: the raw waker owns one Arc reference and this callback
        // temporarily increments it before accessing the shared test state.
        unsafe { Arc::<LocalAbortCleanupState<u8>>::increment_strong_count(data.cast()) };
        let state = unsafe { Arc::<LocalAbortCleanupState<u8>>::from_raw(data.cast()) };
        let mut result = state.result.lock().expect("cleanup result lock");
        *result = Some(CleanupOutcome::Released);
        std::panic::panic_any(());
    }

    unsafe fn drop_arc(data: *const ()) {
        // SAFETY: this is the one reference owned by the raw waker.
        drop(unsafe { Arc::<LocalAbortCleanupState<u8>>::from_raw(data.cast()) });
    }

    fn no_op(_: *const ()) {}

    static PANIC_CLONE_VTABLE: std::task::RawWakerVTable =
        std::task::RawWakerVTable::new(panic_on_clone, no_op, no_op, no_op);

    static PUBLISH_PANIC_VTABLE: std::task::RawWakerVTable =
        std::task::RawWakerVTable::new(publish_then_panic_on_clone, no_op, no_op, drop_arc);

    #[cfg(panic = "unwind")]
    static POISON_RESULT_PANIC_VTABLE: std::task::RawWakerVTable =
        std::task::RawWakerVTable::new(publish_then_poison_result_and_panic_on_clone, no_op, no_op, drop_arc);

    #[test]
    fn test_abort_wait_stays_pending_when_waker_clone_panics() {
        let (mut wait, state) = wait::<u8>();
        // The static vtable uses null data and never dereferences it.
        let hostile = unsafe { Waker::from_raw(std::task::RawWaker::new(std::ptr::null(), &PANIC_CLONE_VTABLE)) };
        let mut context = Context::from_waker(&hostile);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Pending
        ));
        assert!(state.waker.lock().expect("registered waker lock").is_none());

        *state.result.lock().expect("cleanup result lock") = Some(CleanupOutcome::Released);
        let mut normal_context = Context::from_waker(Waker::noop());
        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut normal_context),
            Poll::Ready(CleanupOutcome::Released)
        ));
    }

    #[test]
    fn test_abort_wait_rechecks_result_published_during_waker_clone_panic() {
        let (mut wait, state) = wait::<u8>();
        let data = Arc::into_raw(Arc::clone(&state)).cast_mut().cast::<()>();
        // SAFETY: the clone callback publishes through this Arc and panics;
        // drop releases the initial Arc reference exactly once.
        let hostile = unsafe { Waker::from_raw(std::task::RawWaker::new(data.cast_const(), &PUBLISH_PANIC_VTABLE)) };
        let mut context = Context::from_waker(&hostile);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
        assert!(state.waker.lock().expect("registered waker lock").is_none());
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_abort_wait_recovers_poisoned_result_and_waker_after_hostile_clone() {
        let (mut wait, state) = wait::<u8>();
        let waker_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _guard = waker_state.waker.lock().expect("registered waker lock");
            panic!("poison registered waker lock before hostile clone");
        });
        assert!(poisoner.join().is_err());
        let data = Arc::into_raw(Arc::clone(&state)).cast_mut().cast::<()>();
        // SAFETY: the raw waker owns one Arc reference; its clone callback
        // publishes the result while panicking with the result mutex held.
        let hostile = unsafe {
            Waker::from_raw(std::task::RawWaker::new(
                data.cast_const(),
                &POISON_RESULT_PANIC_VTABLE,
            ))
        };
        let mut context = Context::from_waker(&hostile);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
        assert!(state.result.lock().unwrap_or_else(|error| error.into_inner()).is_none());
        assert!(state.waker.lock().unwrap_or_else(|error| error.into_inner()).is_none());
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_abort_wait_recovers_result_poisoned_during_waker_clone_panic() {
        let (mut wait, state) = wait::<u8>();
        let data = Arc::into_raw(Arc::clone(&state)).cast_mut().cast::<()>();
        // SAFETY: the clone callback poisons the result lock while publishing;
        // drop releases the initial Arc reference exactly once.
        let hostile =
            unsafe { Waker::from_raw(std::task::RawWaker::new(data.cast_const(), &POISON_RESULT_PANIC_VTABLE)) };
        let mut context = Context::from_waker(&hostile);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
        assert!(state.result.lock().unwrap_or_else(|error| error.into_inner()).is_none());
        assert!(state.waker.lock().unwrap_or_else(|error| error.into_inner()).is_none());
    }

    #[test]
    fn test_abort_wait_drops_clone_published_waker_outside_locks_and_contains_panic() {
        let (mut wait, state) = wait::<u8>();
        let probe = PublishOnCloneProbe {
            state,
            drops: AtomicUsize::new(0),
            locks_available: AtomicBool::new(false),
            panic_on_drop: true,
        };
        // SAFETY: the raw waker's data points to a probe kept alive until the poll
        // returns.
        let waker = unsafe {
            Waker::from_raw(std::task::RawWaker::new(
                &probe as *const _ as *const (),
                &PUBLISH_ON_CLONE_VTABLE,
            ))
        };
        let mut context = Context::from_waker(&waker);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
        assert_eq!(1, probe.drops.load(Ordering::SeqCst));
        assert!(probe.locks_available.load(Ordering::SeqCst));
        std::mem::forget(waker);
    }

    #[test]
    fn test_abort_wait_consumes_clone_published_result_with_poisoned_waker_lock() {
        let (mut wait, state) = wait::<u8>();
        let waker_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _guard = waker_state.waker.lock().expect("registered waker lock");
            panic!("poison registered waker lock before result publication");
        });
        assert!(poisoner.join().is_err());
        let probe = PublishOnCloneProbe {
            state,
            drops: AtomicUsize::new(0),
            locks_available: AtomicBool::new(true),
            panic_on_drop: false,
        };
        // SAFETY: the raw waker references the probe, which remains alive
        // until the poll has consumed and dropped its cloned waker.
        let waker = unsafe {
            Waker::from_raw(std::task::RawWaker::new(
                &probe as *const _ as *const (),
                &PUBLISH_ON_CLONE_VTABLE,
            ))
        };
        let mut context = Context::from_waker(&waker);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut context),
            Poll::Ready(CleanupOutcome::Released)
        ));
        assert_eq!(1, probe.drops.load(Ordering::SeqCst));
        assert!(!probe.locks_available.load(Ordering::SeqCst));
        std::mem::forget(waker);
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_abort_wait_recovers_poisoned_waker_after_clone_panic_publishes_result() {
        let (mut wait, state) = wait::<u8>();
        let wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registered = Waker::from(Arc::new(TestWake(Arc::clone(&wake_count))));
        let mut registered_context = Context::from_waker(&registered);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut registered_context),
            Poll::Pending
        ));
        assert!(state.waker.lock().expect("registered waker lock").is_some());
        crate::operation::poison_test_mutex(&state.waker);

        let data = Arc::into_raw(Arc::clone(&state)).cast_mut().cast::<()>();
        // SAFETY: the clone callback publishes through this Arc and panics;
        // drop releases the initial Arc reference exactly once.
        let hostile = unsafe { Waker::from_raw(std::task::RawWaker::new(data.cast_const(), &PUBLISH_PANIC_VTABLE)) };
        let mut hostile_context = Context::from_waker(&hostile);
        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut hostile_context),
            Poll::Ready(CleanupOutcome::Released)
        ));
        assert!(state.waker.lock().unwrap_or_else(|error| error.into_inner()).is_none());
        assert_eq!(0, wake_count.load(std::sync::atomic::Ordering::SeqCst));
    }
}
