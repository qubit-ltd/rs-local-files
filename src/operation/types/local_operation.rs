// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "async")]
use super::AsyncLocalOperation;
use crate::LocalFileError;
/// Synchronous handle for a typed local operation managed by
/// [`qubit_operation`].
///
/// The handle owns the operation's result slot and any local memory budget
/// reserved for it. A successful call to [`wait_outcome`](Self::wait_outcome)
/// or [`try_take_outcome`](Self::try_take_outcome) transfers the outcome to
/// the caller and releases this handle's budget reference. Dropping the
/// outcome is necessary when it owns a resource that should be closed before
/// waiting for settlement. Settlement reports whether the operation and its
/// cleanup have finished; it does not deliver or replace the outcome.
///
/// Requesting stop is also separate from observing the result: a stop request
/// can be rejected or arrive too late, so callers must still observe the
/// outcome. Dropping this handle discards its result slot and releases its
/// budget reference, but does not wait for operation settlement.
///
/// # Example
///
/// This example submits an operation through the public asynchronous
/// filesystem facade, consumes its outcome synchronously, then observes
/// settlement. For operations whose outcome owns a resource, close and drop
/// that resource before waiting for settlement.
///
/// ```no_run
/// use std::path::PathBuf;
///
/// use qubit_local_files::LocalCreateDirectoryRequest;
/// use qubit_local_files::LocalOperationOptions;
/// use qubit_local_files::LocalOperation;
/// use qubit_local_files::operation::AsyncLocalFileSystem;
/// use qubit_local_files::options::LocalCreateDirectoryOptions;
/// use qubit_operation::OperationManager;
/// use qubit_operation::OperationOptions;
/// use qubit_operation::OperationOutcome;
///
/// fn observe_operation(
///     filesystem: &AsyncLocalFileSystem,
///     manager: &OperationManager,
/// ) -> Result<(), Box<dyn std::error::Error>> {
///     let (mut operation, facts): (LocalOperation<_>, _) = filesystem.submit_create_directory(
///         manager,
///         OperationOptions::default(),
///         LocalCreateDirectoryRequest {
///             path: PathBuf::from("created"),
///             options: LocalCreateDirectoryOptions::default(),
///         },
///         LocalOperationOptions::default(),
///     )?;
///     let operation_id = operation.id();
///
///     match operation.wait_outcome()? {
///         OperationOutcome::Succeeded(result) => drop(result),
///         OperationOutcome::Failed(error) => return Err(error.into()),
///         other => return Err(format!("operation ended without a result: {other:?}").into()),
///     }
///     drop(facts);
///     let settlement = operation.wait_settlement()?;
///     assert_eq!(settlement.operation_id, operation_id);
///     Ok(())
/// }
/// ```
///
/// # Type Parameters
/// - `T` — type carried through the operation contract.
/// - `E` — type carried through the operation contract.
pub struct LocalOperation<T: Send + 'static, E: Send + 'static = LocalFileError> {
    /// Stores the inner associated with this value.
    pub(in crate::operation::types) inner: Option<qubit_operation::OperationHandle<T, E>>,
    /// Stores the budget associated with this value.
    pub(in crate::operation::types) budget: Option<Arc<dyn Send + Sync>>,
}

impl<T: Send + 'static, E: Send + 'static> LocalOperation<T, E> {
    /// Runs the new behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `budget` — supplies data or policy for this operation.
    #[cfg(feature = "async")]
    pub(crate) fn new(inner: qubit_operation::OperationHandle<T, E>, budget: Arc<dyn Send + Sync>) -> Self {
        Self {
            inner: Some(inner),
            budget: Some(budget),
        }
    }
    /// Returns this operation's stable identifier.
    pub fn id(&self) -> qubit_operation::OperationId {
        self.inner().id()
    }
    /// Requests that the operation stop for `reason`.
    ///
    /// The returned status describes whether the request was accepted; it
    /// does not indicate the final outcome. Observe the outcome separately.
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
    /// Blocks until the operation outcome is available and takes it from the
    /// result slot.
    ///
    /// On success, this handle releases its owned-memory budget reference
    /// before returning the outcome. An [`OutcomeError`](qubit_operation::OutcomeError)
    /// means the outcome could not be taken; it is not an operation failure
    /// delivered by `T` or `E`.
    pub fn wait_outcome(&mut self) -> Result<qubit_operation::OperationOutcome<T, E>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().wait_outcome();
        if result.is_ok() {
            self.budget.take();
        }
        result
    }
    /// Takes the outcome if it is already available, without blocking.
    ///
    /// `Ok(None)` means the operation has not produced an outcome yet. When
    /// `Ok(Some(_))` is returned, this handle releases its owned-memory budget
    /// reference. An [`OutcomeError`](qubit_operation::OutcomeError) describes
    /// failure to take the result slot, not the operation's `E` value.
    pub fn try_take_outcome(
        &mut self,
    ) -> Result<Option<qubit_operation::OperationOutcome<T, E>>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().try_take_outcome();
        if matches!(result, Ok(Some(_))) {
            self.budget.take();
        }
        result
    }
    /// Blocks until operation execution and cleanup have settled.
    ///
    /// Settlement observation is independent of taking the outcome. The
    /// outcome may already have been consumed, and resource values returned
    /// in it remain the caller's responsibility. An
    /// [`ObservationError`](qubit_operation::ObservationError) indicates that
    /// settlement could not be observed.
    pub fn wait_settlement(&self) -> Result<Arc<qubit_operation::SettlementReport>, qubit_operation::ObservationError> {
        self.inner().wait_settlement()
    }
    /// Waits up to `timeout` for operation execution and cleanup to settle.
    ///
    /// A pending result means the timeout elapsed before settlement; call
    /// again or use [`wait_settlement`](Self::wait_settlement) to wait without
    /// a timeout. This does not consume the operation outcome.
    /// # Parameters
    /// - `timeout` — maximum duration to wait for settlement.
    pub fn wait_settlement_for(
        &self,
        timeout: Duration,
    ) -> Result<qubit_operation::SettlementWait, qubit_operation::ObservationError> {
        self.inner().wait_settlement_for(timeout)
    }
    /// Converts this handle into an observer of operation completion.
    ///
    /// The result slot and this handle's budget reference are released. The
    /// observer can inspect state and settlement but cannot take the outcome.
    pub fn into_observer(mut self) -> qubit_operation::OperationObserver {
        let observer = self.inner.take().expect("local handle is present").into_observer();
        self.budget.take();
        observer
    }
    /// Converts this synchronous handle into its asynchronous counterpart.
    ///
    /// The result slot and budget ownership move to the returned
    /// [`AsyncLocalOperation`].
    #[cfg(feature = "async")]
    pub fn into_async(mut self) -> AsyncLocalOperation<T, E> {
        let inner = self.inner.take().expect("local handle is present").into_async();
        AsyncLocalOperation {
            inner: Some(inner),
            budget: self.budget.take(),
        }
    }
    /// Runs the inner behavior while preserving the operation lifecycle
    /// invariants.
    fn inner(&self) -> &qubit_operation::OperationHandle<T, E> {
        self.inner.as_ref().expect("local handle is present")
    }
    /// Runs the inner mut behavior while preserving the operation lifecycle
    /// invariants.
    fn inner_mut(&mut self) -> &mut qubit_operation::OperationHandle<T, E> {
        self.inner.as_mut().expect("local handle is present")
    }
}

impl<T: Send + 'static, E: Send + 'static> Drop for LocalOperation<T, E> {
    /// Discards any untaken result slot and releases this handle's budget
    /// reference. Dropping does not wait for settlement.
    fn drop(&mut self) {
        drop(self.inner.take());
        self.budget.take();
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "async")]
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    #[cfg(feature = "async")]
    use std::task::Wake;
    #[cfg(feature = "async")]
    use std::task::Waker;

    use qubit_operation::GroupOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::LocalOperation;

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[cfg(feature = "async")]
    struct ThreadWake(std::thread::Thread);

    #[cfg(feature = "async")]
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    #[cfg(feature = "async")]
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
    fn test_sync_handle_exposes_state_and_releases_budget_after_outcome() {
        let group = group();
        let inner = group
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(37_u8))
            .expect("submission");
        let released = Arc::new(AtomicBool::new(false));
        let mut operation = LocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };
        let id = operation.id();

        assert_eq!(operation.snapshot().operation_id, id);
        let _ = operation.request_stop(qubit_operation::StopReason::UserCancelled);
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
        assert!(matches!(
            outcome,
            OperationOutcome::Succeeded(37) | OperationOutcome::Stopped(_)
        ));
        assert!(released.load(Ordering::SeqCst));
        let initial_settlement = operation.wait_settlement_for(std::time::Duration::ZERO).unwrap();
        assert!(matches!(
            initial_settlement,
            qubit_operation::SettlementWait::Ready(_) | qubit_operation::SettlementWait::Pending(_)
        ));
        assert_eq!(operation.wait_settlement().unwrap().operation_id, id);
        assert!(matches!(
            operation.wait_settlement_for(std::time::Duration::ZERO).unwrap(),
            qubit_operation::SettlementWait::Ready(_)
        ));
    }

    #[cfg(feature = "async")]
    #[test]
    fn test_sync_handle_converts_to_async_and_observer_without_losing_identity() {
        let group = group();
        let inner = group
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(41_u8))
            .expect("submission");
        let released = Arc::new(AtomicBool::new(false));
        let operation = LocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };
        let mut asynchronous = operation.into_async();
        let id = asynchronous.id();
        assert_eq!(asynchronous.snapshot().operation_id, id);
        assert!(matches!(
            block_on(asynchronous.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(41)
        ));
        assert!(released.load(Ordering::SeqCst));
        assert_eq!(block_on(asynchronous.wait_settlement()).operation_id, id);

        let inner = group
            .try_submit(OperationOptions::default(), |_| Ok::<_, ()>(43_u8))
            .expect("submission");
        let operation = LocalOperation {
            inner: Some(inner),
            budget: Some(budget(&released)),
        };
        let observer = operation.into_observer();
        assert_eq!(observer.id(), observer.snapshot().operation_id);
        observer.wait_settlement().unwrap();
    }
}
