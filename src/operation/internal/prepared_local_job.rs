// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

mod port;
mod task;

use port::Port;
use qubit_operation::ChildOutcomeKind;
use qubit_operation::ChildOutcomeSummary;
use qubit_operation::ChildRelease;
use qubit_operation::ChildSnapshot;
use qubit_operation::DiagnosticSummary;
use qubit_operation::ExecutionFailure;
use qubit_operation::ExecutionPhase;
use qubit_operation::OperationControl;
use qubit_operation::OperationId;
use qubit_operation::OperationOutcome;
use qubit_operation::OperationScope;
use qubit_operation::PreparedExternalOperation;
use qubit_operation::SettlementReport;
use qubit_operation::SettlementStatus;
use qubit_operation::StopReason;
use task::OperationTask;

use super::super::LocalFactObserver;
use super::super::executor::ExecutorInner;
use super::permits::Permit;
/// Owned job prepared before manager admission. Binding and activation are
/// one-shot.
///
/// # Type Parameters
/// - `R` — owned request retained until admission resolves.
/// - `T` — operation result delivered through the external control port.
/// - `E` — operation error delivered through the external control port.
pub(crate) struct PreparedLocalJob<R, T, E> {
    /// Stores the executor associated with this value.
    executor: Arc<ExecutorInner>,
    /// Stores the request associated with this value.
    request: Mutex<Option<R>>,
    /// Stores the task associated with this value.
    task: Mutex<Option<OperationTask<R, T, E>>>,
    /// Stores the  slot associated with this value.
    _slot: Mutex<Option<Permit>>,
    /// Stores the  owned permit associated with this value.
    _owned_permit: Mutex<Option<Arc<Mutex<Permit>>>>,
    /// Stores the  fact observer associated with this value.
    _fact_observer: Mutex<Option<LocalFactObserver>>,
    /// Stores the  handle permit associated with this value.
    _handle_permit: Mutex<Option<Arc<Permit>>>,
    /// Stores the claimed associated with this value.
    claimed: AtomicBool,
    /// Stores the activation started associated with this value.
    activation_started: AtomicBool,
    /// Stores the port associated with this value.
    port: Mutex<Port<T, E>>,
}

impl<R, T, E> PreparedLocalJob<R, T, E>
where
    R: Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    /// Runs the new behavior while preserving the operation lifecycle
    /// invariants.
    /// # Type Parameters
    /// - `F` — type used by this operation.
    /// # Parameters
    /// - `request` — supplies data or policy for this operation.
    /// - `slot` — supplies data or policy for this operation.
    /// - `owned_permit` — supplies data or policy for this operation.
    /// - `handle_permit` — supplies data or policy for this operation.
    /// - `fact_observer` — supplies data or policy for this operation.
    /// - `task` — supplies data or policy for this operation.
    pub(crate) fn new<F>(
        executor: Arc<ExecutorInner>,
        request: R,
        slot: Permit,
        owned_permit: Arc<Mutex<Permit>>,
        handle_permit: Permit,
        fact_observer: LocalFactObserver,
        task: F,
    ) -> Arc<Self>
    where
        F: FnOnce(R) -> OperationOutcome<T, E> + Send + 'static,
    {
        Self::new_scoped(
            executor,
            request,
            slot,
            owned_permit,
            handle_permit,
            fact_observer,
            move |request, _scope| task(request),
        )
    }
    /// Creates a prepared job whose synchronous worker can inspect its
    /// operation scope.
    /// # Type Parameters
    /// - `F` — owned worker closure that produces the typed operation outcome.
    ///
    /// # Parameters
    /// - `executor` — executor state retaining the operation group and resource
    ///   limits.
    /// - `request` — owned input transferred to the worker after activation.
    /// - `slot` — queue or waiter reservation held until the task starts.
    /// - `owned_permit` — memory reservation retained with the result lifetime.
    /// - `handle_permit` — native-handle reservation retained through cleanup.
    /// - `fact_observer` — local effect snapshot channel associated with the
    ///   job.
    /// - `task` — worker closure that performs the operation.
    pub(crate) fn new_scoped<F>(
        executor: Arc<ExecutorInner>,
        request: R,
        slot: Permit,
        owned_permit: Arc<Mutex<Permit>>,
        handle_permit: Permit,
        fact_observer: LocalFactObserver,
        task: F,
    ) -> Arc<Self>
    where
        F: for<'scope> FnOnce(R, &'scope OperationScope) -> OperationOutcome<T, E> + Send + 'static,
    {
        Self::new_scoped_with_shared_handle_permit(
            executor,
            request,
            slot,
            owned_permit,
            Arc::new(handle_permit),
            fact_observer,
            task,
        )
    }
    /// Creates a job whose native-handle permit can also be retained by a
    /// resource returned from the operation outcome.
    /// # Type Parameters
    /// - `F` — owned worker closure that produces the typed operation outcome.
    ///
    /// # Parameters
    /// - `executor` — executor state retaining the operation group and resource
    ///   limits.
    /// - `request` — owned input transferred to the worker after activation.
    /// - `slot` — queue or waiter reservation held until the task starts.
    /// - `owned_permit` — memory reservation retained with the result lifetime.
    /// - `handle_permit` — native-handle reservation retained through cleanup.
    /// - `fact_observer` — local effect snapshot channel associated with the
    ///   job.
    /// - `task` — worker closure that performs the operation.
    pub(crate) fn new_scoped_with_shared_handle_permit<F>(
        executor: Arc<ExecutorInner>,
        request: R,
        slot: Permit,
        owned_permit: Arc<Mutex<Permit>>,
        handle_permit: Arc<Permit>,
        fact_observer: LocalFactObserver,
        task: F,
    ) -> Arc<Self>
    where
        F: for<'scope> FnOnce(R, &'scope OperationScope) -> OperationOutcome<T, E> + Send + 'static,
    {
        Arc::new(Self {
            executor,
            request: Mutex::new(Some(request)),
            task: Mutex::new(Some(Box::new(task))),
            _slot: Mutex::new(Some(slot)),
            _owned_permit: Mutex::new(Some(owned_permit)),
            _fact_observer: Mutex::new(Some(fact_observer)),
            _handle_permit: Mutex::new(Some(handle_permit)),
            claimed: AtomicBool::new(false),
            activation_started: AtomicBool::new(false),
            port: Mutex::new(Port {
                id: None,
                phase: ExecutionPhase::Queued,
                stop: None,
                outcome: None,
                summary: None,
                released: false,
                settlement: None,
                wakers: [None, None, None, None],
            }),
        })
    }
    /// Runs the take wakers behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `port` — supplies data or policy for this operation.
    fn take_wakers(port: &mut Port<T, E>) -> [Option<Waker>; 4] {
        std::mem::take(&mut port.wakers)
    }
    /// Runs the wake all behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `wakers` — supplies data or policy for this operation.
    fn wake_all(mut wakers: [Option<Waker>; 4]) {
        let mut duplicate = [false; 4];
        for index in 0..wakers.len() {
            let Some(current) = wakers[index].as_ref() else {
                continue;
            };
            duplicate[index] = wakers[..index].iter().flatten().any(|prior| prior.will_wake(current));
        }
        for index in 0..wakers.len() {
            let Some(waker) = wakers[index].take() else {
                continue;
            };
            if !duplicate[index] {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    waker.wake_by_ref();
                }));
            }
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)));
        }
    }
    /// Runs the register behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `port` — supplies data or policy for this operation.
    /// - `slot` — supplies data or policy for this operation.
    /// - `waker` — supplies data or policy for this operation.
    fn register(port: &mut Port<T, E>, slot: usize, waker: &mut Option<Waker>) -> Option<Waker> {
        if port.wakers[slot]
            .as_ref()
            .is_some_and(|previous| previous.will_wake(waker.as_ref().expect("new waker")))
        {
            return None;
        }
        std::mem::replace(&mut port.wakers[slot], waker.take())
    }
    /// Runs the discard waker behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `waker` — supplies data or policy for this operation.
    fn discard_waker(waker: Option<Waker>) {
        if let Some(waker) = waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)));
        }
    }
}

impl<R: Send + 'static, T: Send + 'static, E: Send + 'static> PreparedLocalJob<R, T, E> {
    /// Runs the take request behavior while preserving the operation lifecycle
    /// invariants.
    pub(crate) fn take_request(&self) -> Option<R> {
        self.request.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
    /// Runs the finish released behavior while preserving the operation
    /// lifecycle invariants.
    /// # Parameters
    /// - `id` — supplies data or policy for this operation.
    fn finish_released(&self, id: OperationId) {
        self.task.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.request.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._owned_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._handle_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._fact_observer.lock().unwrap_or_else(|e| e.into_inner()).take();
        let wakers = {
            let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
            port.phase = ExecutionPhase::Finished;
            port.released = true;
            port.settlement = Some(empty_settlement(id));
            Self::take_wakers(&mut port)
        };
        Self::wake_all(wakers);
    }
}
/// Runs the empty settlement behavior while preserving the operation lifecycle
/// invariants.
/// # Parameters
/// - `id` — supplies data or policy for this operation.
fn empty_settlement(id: OperationId) -> Arc<SettlementReport> {
    Arc::new(SettlementReport {
        operation_id: id,
        status: SettlementStatus::Released,
        released_count: 0,
        transferred_count: 0,
        recovery_count: 0,
        children: Arc::from([]),
        children_total: 0,
        children_truncated: false,
        cleanup_failures: Arc::from([]),
        cleanup_failures_total: 0,
        cleanup_failures_truncated: false,
        resource_reports: Arc::from([]),
        resource_reports_total: 0,
        resource_reports_truncated: false,
        transfers: Arc::from([]),
        transfers_total: 0,
        transfers_truncated: false,
        external_settlement: None,
        recoveries: Arc::from([]),
        recoveries_total: 0,
        recoveries_truncated: false,
        effects: Arc::from([]),
        effects_total: 0,
        effect_overflow: 0,
    })
}

impl<R, T, E> OperationControl for PreparedLocalJob<R, T, E>
where
    R: Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    /// Runs the request stop behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `reason` — supplies data or policy for this operation.
    fn request_stop(&self, reason: StopReason) {
        let wakers = {
            let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
            if port.stop.is_none() {
                port.stop = Some(reason);
            }
            Self::take_wakers(&mut port)
        };
        Self::wake_all(wakers);
    }
    /// Runs the snapshot behavior while preserving the operation lifecycle
    /// invariants.
    fn snapshot(&self) -> ChildSnapshot {
        let port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        ChildSnapshot {
            operation_id: port.id.expect("manager binds before observing control"),
            execution: port.phase,
            stop: port.stop.map(|reason| qubit_operation::StopReport {
                reason,
                accepted_at: None,
            }),
            outcome: port.summary.clone(),
            input_released: port.released,
            settlement: port.settlement.clone(),
        }
    }
    /// Runs the poll outcome behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `cx` — supplies data or policy for this operation.
    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<ChildOutcomeSummary> {
        let mut new_waker = Some(cx.waker().clone());
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = &port.summary {
            let value = value.clone();
            drop(port);
            Self::discard_waker(new_waker.take());
            return Poll::Ready(value);
        }
        let old = Self::register(&mut port, 1, &mut new_waker);
        drop(port);
        Self::discard_waker(old);
        Self::discard_waker(new_waker);
        Poll::Pending
    }
    /// Runs the poll release behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `cx` — supplies data or policy for this operation.
    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<ChildRelease> {
        let mut new_waker = Some(cx.waker().clone());
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if port.released {
            let released = ChildRelease {
                operation_id: port.id.expect("manager binds before observing control"),
                confirmed_at: None,
            };
            drop(port);
            Self::discard_waker(new_waker.take());
            return Poll::Ready(released);
        }
        let old = Self::register(&mut port, 2, &mut new_waker);
        drop(port);
        Self::discard_waker(old);
        Self::discard_waker(new_waker);
        Poll::Pending
    }
    /// Runs the poll settlement behavior while preserving the operation
    /// lifecycle invariants.
    /// # Parameters
    /// - `cx` — supplies data or policy for this operation.
    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        let mut new_waker = Some(cx.waker().clone());
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = &port.settlement {
            let value = Arc::clone(value);
            drop(port);
            Self::discard_waker(new_waker.take());
            return Poll::Ready(value);
        }
        let old = Self::register(&mut port, 3, &mut new_waker);
        drop(port);
        Self::discard_waker(old);
        Self::discard_waker(new_waker);
        Poll::Pending
    }
}

impl<R, T, E> PreparedExternalOperation<T, E> for PreparedLocalJob<R, T, E>
where
    R: Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    /// Runs the bind behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `id` — supplies data or policy for this operation.
    fn bind(&self, id: OperationId) -> Result<(), qubit_operation::ExternalBindError> {
        if self
            .claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(qubit_operation::ExternalBindError::AlreadyBound);
        }
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        port.id = Some(id);
        Ok(())
    }
    /// Runs the activate behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `scope` — supplies data or policy for this operation.
    fn activate(self: Arc<Self>, scope: OperationScope) -> Result<(), ExecutionFailure> {
        let id = scope.id();
        if self.activation_started.swap(true, Ordering::AcqRel) {
            return Err(ExecutionFailure {
                operation_id: id,
                summary: DiagnosticSummary::new("local job activation was already attempted"),
                source: None,
            });
        }
        let claimed_id = self.port.lock().unwrap_or_else(|e| e.into_inner()).id;
        if claimed_id != Some(id) {
            self.finish_released(id);
            return Err(ExecutionFailure {
                operation_id: id,
                summary: DiagnosticSummary::new("local job identity mismatch"),
                source: None,
            });
        }
        let Some(task) = self.task.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            self.finish_released(id);
            return Err(ExecutionFailure {
                operation_id: id,
                summary: DiagnosticSummary::new("local job was already activated"),
                source: None,
            });
        };
        let this = Arc::clone(&self);
        let request = self.request.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(request) = request else {
            self.finish_released(id);
            return Err(ExecutionFailure {
                operation_id: id,
                summary: DiagnosticSummary::new("local request already taken"),
                source: None,
            });
        };
        let scope_guard = scope;
        let job = Box::new(move || {
            this._slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            let (stop_before_start, wakers) = {
                let mut port = this.port.lock().unwrap_or_else(|e| e.into_inner());
                let stop = if let Some(reason) = port.stop {
                    Some(reason)
                } else {
                    port.phase = ExecutionPhase::Running;
                    None
                };
                (stop, Self::take_wakers(&mut port))
            };
            Self::wake_all(wakers);
            if let Some(reason) = stop_before_start {
                let outcome = OperationOutcome::Stopped(qubit_operation::StopReport {
                    reason,
                    accepted_at: None,
                });
                this._handle_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
                let wakers = {
                    this._fact_observer.lock().unwrap_or_else(|e| e.into_inner()).take();
                    let mut port = this.port.lock().unwrap_or_else(|e| e.into_inner());
                    port.phase = ExecutionPhase::Finished;
                    port.summary = Some(outcome_summary(id, &outcome));
                    port.outcome = Some(outcome);
                    port.released = true;
                    port.settlement = Some(empty_settlement(id));
                    Self::take_wakers(&mut port)
                };
                Self::wake_all(wakers);
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(request, &scope_guard)));
            this._handle_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(_) => OperationOutcome::ExecutionFailed(ExecutionFailure {
                    operation_id: id,
                    summary: DiagnosticSummary::new("local worker job panicked"),
                    source: None,
                }),
            };
            let summary = outcome_summary(id, &outcome);
            let settlement = empty_settlement(id);
            let wakers = {
                this._fact_observer.lock().unwrap_or_else(|e| e.into_inner()).take();
                let mut port = this.port.lock().unwrap_or_else(|e| e.into_inner());
                port.phase = ExecutionPhase::Finished;
                port.summary = Some(summary);
                port.outcome = Some(outcome);
                port.released = true;
                port.settlement = Some(settlement);
                Self::take_wakers(&mut port)
            };
            Self::wake_all(wakers);
        });
        if let Err(job) = self.executor.queue.push(
            job,
            false,
            self.executor
                .config
                .queue_capacity
                .saturating_add(self.executor.config.waiter_capacity),
        ) {
            drop(job);
            self.finish_released(id);
            return Err(ExecutionFailure {
                operation_id: id,
                summary: DiagnosticSummary::new("local worker queue is full"),
                source: None,
            });
        }
        Ok(())
    }
    /// Runs the abandon behavior while preserving the operation lifecycle
    /// invariants.
    fn abandon(self: Arc<Self>) {
        self.task.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.request.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._owned_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._handle_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._fact_observer.lock().unwrap_or_else(|e| e.into_inner()).take();
        let wakers = {
            let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(id) = port.id {
                port.phase = ExecutionPhase::Finished;
                port.released = true;
                port.settlement = Some(empty_settlement(id));
            }
            Self::take_wakers(&mut port)
        };
        Self::wake_all(wakers);
    }
    /// Runs the poll take outcome behavior while preserving the operation
    /// lifecycle invariants.
    /// # Parameters
    /// - `cx` — supplies data or policy for this operation.
    fn poll_take_outcome(&self, cx: &mut Context<'_>) -> Poll<OperationOutcome<T, E>> {
        let mut new_waker = Some(cx.waker().clone());
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = port.outcome.take() {
            drop(port);
            self._owned_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
            Self::discard_waker(new_waker.take());
            return Poll::Ready(value);
        }
        let old = Self::register(&mut port, 0, &mut new_waker);
        drop(port);
        Self::discard_waker(old);
        Self::discard_waker(new_waker);
        Poll::Pending
    }
}
/// Runs the outcome summary behavior while preserving the operation lifecycle
/// invariants.
/// # Type Parameters
/// - `T` — type used by this operation.
/// - `E` — type used by this operation.
/// # Parameters
/// - `id` — supplies data or policy for this operation.
/// - `outcome` — supplies data or policy for this operation.
fn outcome_summary<T, E>(id: OperationId, outcome: &OperationOutcome<T, E>) -> ChildOutcomeSummary {
    let (kind, text) = match outcome {
        OperationOutcome::Succeeded(_) => (ChildOutcomeKind::Succeeded, "child succeeded"),
        OperationOutcome::Failed(_) | OperationOutcome::ChildFailed(_) => (ChildOutcomeKind::Failed, "child failed"),
        OperationOutcome::Stopped(_) => (ChildOutcomeKind::Stopped, "child stopped"),
        OperationOutcome::Panicked(panic) => (ChildOutcomeKind::Panicked, panic.summary.as_str()),
        OperationOutcome::ExecutionFailed(error) => (ChildOutcomeKind::ExecutionFailed, error.summary.as_str()),
    };
    ChildOutcomeSummary {
        operation_id: id,
        kind,
        summary: DiagnosticSummary::new(text),
    }
}

#[cfg(test)]
mod outcome_summary_tests {
    use qubit_operation::ChildFailureSummary;
    use qubit_operation::ChildOutcomeKind;
    use qubit_operation::ChildOutcomeSummary;
    use qubit_operation::DiagnosticSummary;
    use qubit_operation::ExecutionFailure;
    use qubit_operation::ExecutionPhase;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PanicSummary;
    use qubit_operation::StopReason;
    use qubit_operation::StopReport;

    use super::outcome_summary;

    #[test]
    fn test_outcome_summary_preserves_each_child_result_category() {
        let manager = OperationManager::new(Default::default()).expect("operation manager");
        let id = manager
            .try_submit(OperationOptions::default(), |_| Ok::<(), ()>(()))
            .expect("operation admitted")
            .id();
        let failed_child = ChildFailureSummary {
            operation_id: id,
            outcome: ChildOutcomeSummary {
                operation_id: id,
                kind: ChildOutcomeKind::Failed,
                summary: DiagnosticSummary::new("nested failure"),
            },
        };
        let panic = PanicSummary {
            operation_id: id,
            phase: ExecutionPhase::Running,
            summary: DiagnosticSummary::new("worker panic"),
        };
        let execution_failure = ExecutionFailure {
            operation_id: id,
            summary: DiagnosticSummary::new("executor failure"),
            source: None,
        };

        let cases = [
            (
                OperationOutcome::<(), ()>::Succeeded(()),
                ChildOutcomeKind::Succeeded,
                "child succeeded",
            ),
            (
                OperationOutcome::<(), ()>::Failed(()),
                ChildOutcomeKind::Failed,
                "child failed",
            ),
            (
                OperationOutcome::<(), ()>::ChildFailed(failed_child),
                ChildOutcomeKind::Failed,
                "child failed",
            ),
            (
                OperationOutcome::<(), ()>::Stopped(StopReport {
                    reason: StopReason::UserCancelled,
                    accepted_at: None,
                }),
                ChildOutcomeKind::Stopped,
                "child stopped",
            ),
            (
                OperationOutcome::<(), ()>::Panicked(panic),
                ChildOutcomeKind::Panicked,
                "worker panic",
            ),
            (
                OperationOutcome::<(), ()>::ExecutionFailed(execution_failure),
                ChildOutcomeKind::ExecutionFailed,
                "executor failure",
            ),
        ];

        for (outcome, expected_kind, expected_summary) in cases {
            let summary = outcome_summary(id, &outcome);
            assert_eq!(summary.operation_id, id);
            assert_eq!(summary.kind, expected_kind);
            assert_eq!(summary.summary.as_str(), expected_summary);
        }
    }
}

#[cfg(test)]
mod waker_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ExecutionPhase;

    use super::Port;
    use super::PreparedLocalJob;

    struct CountWake(AtomicUsize);

    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn empty_port() -> Port<(), ()> {
        Port {
            id: None,
            phase: ExecutionPhase::Queued,
            stop: None,
            outcome: None,
            summary: None,
            released: false,
            settlement: None,
            wakers: std::array::from_fn(|_| None),
        }
    }

    #[test]
    fn test_register_replaces_the_previous_waker() {
        let first = Arc::new(CountWake(AtomicUsize::new(0)));
        let second = Arc::new(CountWake(AtomicUsize::new(0)));
        let first_waker = Waker::from(Arc::clone(&first));
        let mut port = empty_port();
        let mut pending = Some(first_waker);

        assert!(PreparedLocalJob::<(), (), ()>::register(&mut port, 1, &mut pending).is_none());
        assert!(pending.is_none());

        let mut replacement = Some(Waker::from(second));
        let old = PreparedLocalJob::<(), (), ()>::register(&mut port, 1, &mut replacement);
        assert!(old.is_some());
        assert!(replacement.is_none());
        PreparedLocalJob::<(), (), ()>::discard_waker(old);
    }

    #[test]
    fn test_wake_all_wakes_registered_waiters_and_clears_slots() {
        let first = Arc::new(CountWake(AtomicUsize::new(0)));
        let second = Arc::new(CountWake(AtomicUsize::new(0)));
        let mut port = empty_port();
        port.wakers[0] = Some(Waker::from(Arc::clone(&first)));
        port.wakers[2] = Some(Waker::from(Arc::clone(&second)));

        let wakers = PreparedLocalJob::<(), (), ()>::take_wakers(&mut port);
        assert!(port.wakers.iter().all(Option::is_none));
        PreparedLocalJob::<(), (), ()>::wake_all(wakers);

        assert_eq!(first.0.load(Ordering::SeqCst), 1);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_owned_waker_wake_notifies_the_registered_target() {
        let notified = Arc::new(CountWake(AtomicUsize::new(0)));
        Waker::from(Arc::clone(&notified)).wake();
        assert_eq!(notified.0.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod activation_state_tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PreparedExternalOperation;

    use super::PreparedLocalJob;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalFactObserver;
    use crate::operation::LocalIoExecutor;

    #[test]
    fn test_activation_rejects_a_prepared_job_without_its_task() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let mut operation = manager
            .try_submit(Default::default(), move |scope| {
                let job = PreparedLocalJob::new(
                    Arc::clone(&executor.inner),
                    (),
                    executor.reserve_slot().expect("queue slot"),
                    Arc::new(Mutex::new(executor.reserve_bytes(0).expect("owned permit"))),
                    executor.reserve_handle().expect("handle permit"),
                    LocalFactObserver::new(),
                    |_| OperationOutcome::<(), ()>::Succeeded(()),
                );
                job.task.lock().unwrap_or_else(|error| error.into_inner()).take();
                PreparedExternalOperation::bind(&*job, scope.id()).expect("bind job identity");
                let failure = PreparedExternalOperation::activate(job.clone(), scope)
                    .expect_err("activation reports a job whose one-shot task was consumed");
                assert_eq!("local job was already activated", failure.summary.as_str());
                assert!(qubit_operation::OperationControl::snapshot(&*job).input_released);
                Ok::<_, ()>(())
            })
            .expect("parent operation");
        assert!(matches!(
            operation.wait_outcome().expect("parent outcome"),
            OperationOutcome::Succeeded(())
        ));
    }
}
