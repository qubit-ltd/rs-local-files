// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::AsyncResult;
use super::safe_wake;

/// Completes one result cell and wakes its waiting receiver.
///
/// # Type Parameters
///
/// * `T` — result value delivered to the receiver.
pub(super) struct AsyncSender<T>(
    /// Shared result cell completed by this one-shot sender.
    pub(super) Arc<AsyncResult<T>>,
);
impl<T> AsyncSender<T> {
    /// Publishes the value and notifies the receiver, if it has been polled.
    ///
    /// # Parameters
    ///
    /// * `self` — one-shot sender consumed by completion.
    /// * `value` — result value transferred to the waiting receiver.
    pub(super) fn send(self, value: T) -> Result<(), T> {
        *self.0.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
        let waker = self.0.waker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(waker) = waker {
            safe_wake(waker);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Waker;

    use super::super::async_result;

    #[test]
    fn test_sender_recovers_a_poisoned_result_lock() {
        let (sender, mut receiver) = async_result::<u8>();
        let state = Arc::clone(&sender.0);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.value.lock().expect("result lock");
            panic!("poison result lock");
        }));
        assert!(state.value.is_poisoned());

        assert!(sender.send(11).is_ok());
        let mut context = std::task::Context::from_waker(Waker::noop());
        assert!(matches!(
            std::pin::Pin::new(&mut receiver).poll(&mut context),
            std::task::Poll::Ready(11)
        ));
    }

    #[test]
    fn test_sender_recovers_a_poisoned_waker_lock() {
        let (sender, mut receiver) = async_result::<u8>();
        let state = Arc::clone(&sender.0);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.waker.lock().expect("waker lock");
            panic!("poison waker lock");
        }));
        assert!(state.waker.is_poisoned());

        assert!(sender.send(13).is_ok());
        let mut context = std::task::Context::from_waker(Waker::noop());
        assert!(matches!(
            std::pin::Pin::new(&mut receiver).poll(&mut context),
            std::task::Poll::Ready(13)
        ));
    }
}
