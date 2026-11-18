// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "async")]
use std::sync::Arc;

#[cfg(feature = "async")]
use super::LocalOperation;
#[cfg(feature = "async")]
use crate::LocalFileError;
/// Owns an asynchronous local operation result and its memory reservation.
///
/// # Type Parameters
/// - `T` — result type delivered by the operation.
/// - `E` — error type delivered when the operation fails.
#[cfg(feature = "async")]
pub struct AsyncLocalOperation<T: Send + 'static, E: Send + 'static = LocalFileError> {
    /// Stores the inner associated with this value.
    pub(in crate::operation::types) inner: Option<qubit_operation::AsyncOperationHandle<T, E>>,
    /// Stores the budget associated with this value.
    pub(in crate::operation::types) budget: Option<Arc<dyn Send + Sync>>,
}

#[cfg(feature = "async")]
impl<T: Send + 'static, E: Send + 'static> AsyncLocalOperation<T, E> {
    /// Runs the id behavior while preserving the operation lifecycle
    /// invariants.
    pub fn id(&self) -> qubit_operation::OperationId {
        self.inner().id()
    }
    /// Runs the request stop behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `reason` — supplies data or policy for this operation.
    pub fn request_stop(&self, reason: qubit_operation::StopReason) -> qubit_operation::StopRequestStatus {
        self.inner().request_stop(reason)
    }
    /// Runs the snapshot behavior while preserving the operation lifecycle
    /// invariants.
    pub fn snapshot(&self) -> qubit_operation::OperationSnapshot {
        self.inner().snapshot()
    }
    /// Runs the wait outcome behavior while preserving the operation lifecycle
    /// invariants.
    pub async fn wait_outcome(
        &mut self,
    ) -> Result<qubit_operation::OperationOutcome<T, E>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().wait_outcome().await;
        if result.is_ok() {
            self.budget.take();
        }
        result
    }
    /// Runs the try take outcome behavior while preserving the operation
    /// lifecycle invariants.
    pub fn try_take_outcome(
        &mut self,
    ) -> Result<Option<qubit_operation::OperationOutcome<T, E>>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().try_take_outcome();
        if matches!(result, Ok(Some(_))) {
            self.budget.take();
        }
        result
    }
    /// Runs the wait settlement behavior while preserving the operation
    /// lifecycle invariants.
    pub async fn wait_settlement(&self) -> Arc<qubit_operation::SettlementReport> {
        self.inner().wait_settlement().await
    }
    /// Runs the into sync behavior while preserving the operation lifecycle
    /// invariants.
    pub fn into_sync(mut self) -> LocalOperation<T, E> {
        let inner = self.inner.take().expect("async local handle is present").into_sync();
        LocalOperation {
            inner: Some(inner),
            budget: self.budget.take(),
        }
    }
    /// Runs the into observer behavior while preserving the operation lifecycle
    /// invariants.
    pub fn into_observer(mut self) -> qubit_operation::AsyncOperationObserver {
        let observer = self
            .inner
            .take()
            .expect("async local handle is present")
            .into_observer();
        self.budget.take();
        observer
    }
    /// Runs the inner behavior while preserving the operation lifecycle
    /// invariants.
    fn inner(&self) -> &qubit_operation::AsyncOperationHandle<T, E> {
        self.inner.as_ref().expect("async local handle is present")
    }
    /// Runs the inner mut behavior while preserving the operation lifecycle
    /// invariants.
    fn inner_mut(&mut self) -> &mut qubit_operation::AsyncOperationHandle<T, E> {
        self.inner.as_mut().expect("async local handle is present")
    }
}

#[cfg(feature = "async")]
impl<T: Send + 'static, E: Send + 'static> Drop for AsyncLocalOperation<T, E> {
    /// Runs the drop behavior while preserving the operation lifecycle
    /// invariants.
    fn drop(&mut self) {
        drop(self.inner.take());
        self.budget.take();
    }
}

#[cfg(all(test, feature = "async"))]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::GroupOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::AsyncLocalOperation;

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct ThreadWake(std::thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = std::task::Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(value) => return value,
                std::task::Poll::Pending => std::thread::park(),
            }
        }
    }

    fn group() -> qubit_operation::OperationGroup {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        manager.group(GroupOptions { capacity: 4 }).expect("operation group")
    }

    fn budget(flag: &Arc<AtomicBool>) -> Arc<dyn Send + Sync> {
        Arc::new(DropFlag(Arc::clone(flag)))
    }

    #[test]
    fn test_async_handle_waits_for_outcome_and_settlement_then_releases_budget() {
        let group = group();
        let inner = block_on(group.try_submit_async(OperationOptions::default(), |_| async { Ok::<_, ()>(17_u8) }))
            .expect("submission");
        let released = Arc::new(AtomicBool::new(false));
        let mut operation = AsyncLocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };

        let id = operation.id();
        assert_eq!(operation.snapshot().operation_id, id);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let outcome = loop {
            match operation.try_take_outcome().unwrap() {
                Some(outcome) => break outcome,
                None => {
                    assert!(!released.load(Ordering::SeqCst));
                    assert!(std::time::Instant::now() < deadline, "operation did not finish");
                    std::thread::yield_now();
                }
            }
        };
        assert!(matches!(outcome, OperationOutcome::Succeeded(17)));
        assert!(released.load(Ordering::SeqCst));
        assert_eq!(block_on(operation.wait_settlement()).operation_id, id);
    }

    #[test]
    fn test_async_handle_wait_outcome_preserves_failure_and_releases_budget() {
        let group = group();
        let inner = block_on(group.try_submit_async(OperationOptions::default(), |_| async {
            Err::<u8, _>("expected failure")
        }))
        .expect("submission");
        let released = Arc::new(AtomicBool::new(false));
        let mut operation = AsyncLocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };

        assert!(matches!(
            block_on(operation.wait_outcome()).expect("outcome delivery"),
            OperationOutcome::Failed("expected failure")
        ));
        let _ = operation.request_stop(qubit_operation::StopReason::UserCancelled);
        assert!(released.load(Ordering::SeqCst));
    }

    #[test]
    fn test_async_handle_conversions_transfer_budget_and_observer_ownership() {
        let group = group();
        let inner = block_on(group.try_submit_async(OperationOptions::default(), |_| async { Ok::<_, ()>(23_u8) }))
            .expect("submission");
        let released = Arc::new(AtomicBool::new(false));
        let operation = AsyncLocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };
        let mut sync = operation.into_sync();
        assert!(matches!(sync.wait_outcome().unwrap(), OperationOutcome::Succeeded(23)));
        assert!(released.load(Ordering::SeqCst));
        sync.wait_settlement().unwrap();

        let inner = block_on(group.try_submit_async(OperationOptions::default(), |_| async { Ok::<_, ()>(29_u8) }))
            .expect("submission");
        let released = Arc::new(AtomicBool::new(false));
        let operation = AsyncLocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };
        let observer = operation.into_observer();
        assert!(released.load(Ordering::SeqCst));
        assert_eq!(block_on(observer.wait_settlement()).operation_id, observer.id());
    }

    #[test]
    fn test_dropping_async_handle_releases_budget_and_requests_operation_drop() {
        let group = group();
        let inner = block_on(group.try_submit_async(OperationOptions::default(), |_| async { Ok::<_, ()>(31_u8) }))
            .expect("submission");
        let released = Arc::new(AtomicBool::new(false));
        let operation = AsyncLocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };
        drop(operation);
        assert!(released.load(Ordering::SeqCst));
    }
}
