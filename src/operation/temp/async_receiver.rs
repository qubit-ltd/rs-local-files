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

use super::AsyncResult;
use super::safe_clone_waker;
use super::safe_drop_waker;

/// Future that consumes the one-shot result published by a worker.
///
/// # Type Parameters
///
/// * `T` — result returned when the worker completes.
pub(super) struct AsyncReceiver<T>(
    /// Shared result cell polled until the worker publishes a value.
    pub(super) Arc<AsyncResult<T>>,
);
impl<T> Future for AsyncReceiver<T> {
    /// One result value published by the worker.
    type Output = T;
    /// Takes a completed result or registers this task for worker notification.
    ///
    /// # Parameters
    ///
    /// * `self` — pinned receiver future that owns the completion cell.
    /// * `cx` — task context used to register a wakeup.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        if let Some(value) = self.0.value.lock().unwrap_or_else(|e| e.into_inner()).take() {
            return Poll::Ready(value);
        }
        let Some(new_waker) = safe_clone_waker(cx.waker()) else {
            return self
                .0
                .value
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .map_or(Poll::Pending, Poll::Ready);
        };
        let old_waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(new_waker);
        safe_drop_waker(old_waker);
        if let Some(value) = self.0.value.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Poll::Ready(value)
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use super::super::async_result;

    struct CountWake(Arc<AtomicUsize>);

    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_receiver_replaces_waiter_and_delivers_one_published_result() {
        let (sender, mut receiver) = async_result::<u8>();
        let previous_wakes = Arc::new(AtomicUsize::new(0));
        let latest_wakes = Arc::new(AtomicUsize::new(0));
        let previous_waker = Waker::from(Arc::new(CountWake(Arc::clone(&previous_wakes))));
        let latest_waker = Waker::from(Arc::new(CountWake(Arc::clone(&latest_wakes))));
        let mut previous_context = std::task::Context::from_waker(&previous_waker);
        let mut latest_context = std::task::Context::from_waker(&latest_waker);

        assert!(
            std::pin::Pin::new(&mut receiver)
                .poll(&mut previous_context)
                .is_pending()
        );
        assert!(
            std::pin::Pin::new(&mut receiver)
                .poll(&mut previous_context)
                .is_pending()
        );
        assert!(std::pin::Pin::new(&mut receiver).poll(&mut latest_context).is_pending());
        assert!(sender.send(42).is_ok());

        assert_eq!(previous_wakes.load(Ordering::SeqCst), 0);
        assert_eq!(latest_wakes.load(Ordering::SeqCst), 1);
        assert!(matches!(
            std::pin::Pin::new(&mut receiver).poll(&mut latest_context),
            std::task::Poll::Ready(42)
        ));
    }

    #[test]
    fn test_receiver_takes_a_result_that_arrived_before_first_poll() {
        let (sender, mut receiver) = async_result::<u8>();
        assert!(sender.send(7).is_ok());
        let mut context = std::task::Context::from_waker(Waker::noop());

        assert!(matches!(
            std::pin::Pin::new(&mut receiver).poll(&mut context),
            std::task::Poll::Ready(7)
        ));
        assert!(std::pin::Pin::new(&mut receiver).poll(&mut context).is_pending());
    }

    #[test]
    fn test_sender_transfers_an_owned_string_through_the_waiting_receiver() {
        let (sender, mut receiver) = async_result::<String>();
        let mut context = std::task::Context::from_waker(Waker::noop());
        assert!(std::pin::Pin::new(&mut receiver).poll(&mut context).is_pending());
        assert!(sender.send("completed worker result".to_owned()).is_ok());
        assert!(matches!(
            std::pin::Pin::new(&mut receiver).poll(&mut context),
            std::task::Poll::Ready(value) if value == "completed worker result"
        ));
    }
}
