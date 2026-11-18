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
        if let Some(result) = self.0.result.lock().unwrap_or_else(|error| error.into_inner()).take() {
            return Poll::Ready(result);
        }
        let Some(new_waker) = super::super::temp::safe_clone_waker(context.waker()) else {
            return Poll::Pending;
        };
        let old_waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(new_waker);
        super::super::temp::safe_drop_waker(old_waker);
        match self.0.result.lock().unwrap_or_else(|error| error.into_inner()).take() {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Waker;

    use qubit_operation::CleanupOutcome;

    use super::super::LocalAbortCleanupState;
    use super::LocalAbortCleanupWait;

    fn wait<W: Send + 'static>() -> (LocalAbortCleanupWait<W>, Arc<LocalAbortCleanupState<W>>) {
        let state = Arc::new(LocalAbortCleanupState {
            owner: Mutex::new(None),
            result: Mutex::new(None),
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
        let wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let first_waker = Waker::from(Arc::new(TestWake(Arc::clone(&wake_count))));
        let mut first_context = Context::from_waker(&first_waker);

        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut first_context),
            Poll::Pending
        ));

        let replacement_waker = Waker::from(Arc::new(TestWake(Arc::clone(&wake_count))));
        let mut replacement_context = Context::from_waker(&replacement_waker);
        assert!(matches!(
            Future::poll(std::pin::Pin::new(&mut wait), &mut replacement_context),
            Poll::Pending
        ));

        let registered_waker = state.waker.lock().expect("registered waker lock");
        assert!(
            registered_waker
                .as_ref()
                .is_some_and(|waker| waker.will_wake(&replacement_waker))
        );
        assert_eq!(0, wake_count.load(std::sync::atomic::Ordering::SeqCst));
    }
}
