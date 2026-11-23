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
/// Asynchronous handle for the single outcome of a local operation.
///
/// The handle owns the operation's result slot and its owned-memory budget
/// reference. [`wait_outcome`](Self::wait_outcome) takes the outcome, while
/// [`try_take_outcome`](Self::try_take_outcome) takes it only when ready. Once
/// either method returns an outcome, the handle releases its budget reference;
/// the returned value is then owned by the caller. Dropping an untaken handle
/// discards its result slot and releases its reference, without waiting for
/// worker cleanup or closing resources already returned to the caller.
///
/// Outcome delivery and settlement are separate. An outcome can contain a
/// reader, writer, or walker whose native resource remains open after the
/// outcome is delivered. Close and drop such a resource before waiting for
/// settlement when settlement depends on that resource being released.
/// [`wait_settlement`](Self::wait_settlement) observes operation cleanup; it
/// does not take, restore, or dispose of the outcome.
///
/// A stop request is cooperative. Its status reports whether the request was
/// accepted, not whether the operation stopped; always observe the outcome.
/// `OutcomeError` means the result slot could not be taken, rather than an
/// operation failure in `E`.
///
/// # Example
///
/// Submit a read through the public facade, take its asynchronous outcome,
/// then observe settlement. The example compiles without requiring the input
/// file to exist at documentation-test time.
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use std::path::PathBuf;
/// use qubit_local_files::{AsyncLocalFileSystem, LocalExecutionConfig, LocalIoExecutor, LocalReadRequest};
/// use qubit_local_files::operation::{AsyncLocalOperation, LocalOperationOptions};
/// use qubit_operation::{ManagerOptions, OperationManager, OperationOptions, OperationOutcome};
///
/// let manager = OperationManager::new(ManagerOptions::default())?;
/// let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default())?;
/// let filesystem = AsyncLocalFileSystem::host(executor).await?;
/// let (operation, _facts) = filesystem.submit_read_all(
///     &manager,
///     OperationOptions::default(),
///     LocalReadRequest { path: PathBuf::from("input.txt"), read: Default::default() },
///     LocalOperationOptions::default(),
/// )?;
/// let mut operation: AsyncLocalOperation<_> = operation.into_async();
/// match operation.wait_outcome().await? {
///     OperationOutcome::Succeeded(bytes) => println!("read {} bytes", bytes.len()),
///     OperationOutcome::Failed(error) => return Err(error.into()),
///     other => return Err(format!("read did not succeed: {other:?}").into()),
/// }
/// let settlement = operation.wait_settlement().await;
/// println!("settled {:?}", settlement.status);
/// # Ok(())
/// # }
/// ```
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
    /// Returns this operation's stable identifier.
    pub fn id(&self) -> qubit_operation::OperationId {
        self.inner().id()
    }
    /// Requests cooperative stop for `reason`.
    ///
    /// The returned status describes request acceptance, not the final
    /// outcome. The worker may already have completed or may be unable to stop
    /// immediately; take the outcome separately.
    ///
    /// # Parameters
    /// - `reason` — reason to attach to the stop request.
    pub fn request_stop(&self, reason: qubit_operation::StopReason) -> qubit_operation::StopRequestStatus {
        self.inner().request_stop(reason)
    }
    /// Returns a point-in-time snapshot of the operation's state.
    pub fn snapshot(&self) -> qubit_operation::OperationSnapshot {
        self.inner().snapshot()
    }
    /// Waits asynchronously for and takes the operation outcome.
    ///
    /// On success, this handle releases its owned-memory budget reference
    /// before returning the outcome. The caller owns the returned value and
    /// must close any resource it contains. An
    /// [`OutcomeError`](qubit_operation::OutcomeError) reports failure to take
    /// the result slot; an operation failure is returned as
    /// `Ok(Outcome::Failed(E))`.
    pub async fn wait_outcome(
        &mut self,
    ) -> Result<qubit_operation::OperationOutcome<T, E>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().wait_outcome().await;
        if result.is_ok() {
            self.budget.take();
        }
        result
    }
    /// Takes the outcome if it is ready, without waiting.
    ///
    /// `Ok(None)` means no outcome is ready. `Ok(Some(_))` transfers ownership
    /// of the outcome to the caller and releases this handle's memory-budget
    /// reference. `OutcomeError` concerns taking the result slot, not the
    /// operation's error value `E`.
    pub fn try_take_outcome(
        &mut self,
    ) -> Result<Option<qubit_operation::OperationOutcome<T, E>>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().try_take_outcome();
        if matches!(result, Ok(Some(_))) {
            self.budget.take();
        }
        result
    }
    /// Waits asynchronously until operation execution and cleanup settle.
    ///
    /// Settlement observation does not consume the outcome or close resources
    /// returned by it. If a returned reader, writer, or walker keeps cleanup
    /// pending, close and drop it before awaiting settlement.
    pub async fn wait_settlement(&self) -> Arc<qubit_operation::SettlementReport> {
        self.inner().wait_settlement().await
    }
    /// Converts this handle into its blocking counterpart, moving the result
    /// slot and memory-budget ownership to the returned handle.
    pub fn into_sync(mut self) -> LocalOperation<T, E> {
        let inner = self.inner.take().expect("async local handle is present").into_sync();
        LocalOperation {
            inner: Some(inner),
            budget: self.budget.take(),
        }
    }
    /// Converts this handle into a read-only completion observer.
    ///
    /// The result slot and this handle's budget reference are released. The
    /// observer can inspect state and settlement but cannot take the outcome.
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
