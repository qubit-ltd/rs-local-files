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
use super::super::stream::dispose_panic_payload;
use super::permits::Permit;
/// Owned job prepared before manager admission. Binding and activation are
/// one-shot.
///
/// The job publishes one typed outcome for the external handle to take, then
/// separately publishes settlement after execution and owned cleanup finish.
/// Publishing an outcome does not itself mean that a resource returned in the
/// outcome has been closed. Queue, byte, native-handle, and fact-record permits
/// follow their respective owners: resource permits remain with returned
/// resources until close, and the job's internal fact-observer reference is
/// dropped before the outcome is published. The fact-record slot returns only
/// when the last observer clone is dropped. Stop requests are cooperative;
/// the resulting outcome remains the authoritative report of how execution
/// ended.
///
/// # Type Parameters
/// - `R` — owned request retained until admission resolves.
/// - `T` — operation result delivered through the external control port.
/// - `E` — operation error delivered through the external control port.
pub(crate) struct PreparedLocalJob<R, T, E> {
    /// Executor state owning the admission group and shared resource limits.
    executor: Arc<ExecutorInner>,
    /// Request retained until activation transfers it to the worker.
    request: Mutex<Option<R>>,
    /// One-shot worker closure, consumed on activation.
    task: Mutex<Option<OperationTask<R, T, E>>>,
    /// Queue or waiter permit, released when the worker starts or the job is
    /// abandoned.
    _slot: Mutex<Option<Permit>>,
    /// Shared owned-byte reservation retained with the result lifetime.
    _owned_permit: Mutex<Option<Arc<Mutex<Permit>>>>,
    /// Internal observer clone, released before publishing the outcome or
    /// when the job is abandoned.
    _fact_observer: Mutex<Option<LocalFactObserver>>,
    /// Shared native-handle reservation, also retained by returned resources
    /// when applicable.
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
    /// Prepares a one-shot worker job with the supplied request, permits, and
    /// fact observer. The job retains these owners until their lifecycle
    /// boundary; its internal observer is released before outcome publication.
    /// # Type Parameters
    /// - `F` — type used by this operation.
    /// # Parameters
    /// - `request` — input retained until activation transfers it to the
    ///   worker.
    /// - `slot` — queue or waiter capacity released when work starts.
    /// - `owned_permit` — shared memory reservation retained with the result.
    /// - `handle_permit` — native-handle reservation retained through cleanup.
    /// - `fact_observer` — internal observer reference released before the
    ///   outcome is published.
    /// - `task` — one-shot synchronous worker closure.
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
    /// - `fact_observer` — internal observer reference released before the
    ///   outcome is published or when the job is abandoned.
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
    /// - `fact_observer` — internal observer reference released before the
    ///   outcome is published or when the job is abandoned.
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
    /// Takes all registered wakers from the operation port.
    /// # Parameters
    /// - `port` — port whose registered wakers are removed.
    fn take_wakers(port: &mut Port<T, E>) -> [Option<Waker>; 4] {
        std::mem::take(&mut port.wakers)
    }
    /// Wakes distinct registered wakers and safely drops every registration.
    /// # Parameters
    /// - `wakers` — wakers to invoke and then drop.
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
            if !duplicate[index]
                && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    waker.wake_by_ref();
                }))
            {
                dispose_panic_payload(payload);
            }
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker))) {
                dispose_panic_payload(payload);
            }
        }
    }
    /// Replaces a poll slot's waker unless it already wakes the same task.
    /// # Parameters
    /// - `port` — port holding the poll waker slots.
    /// - `slot` — index of the slot for this poll operation.
    /// - `waker` — newly cloned waker, consumed when it replaces the slot.
    fn register(port: &mut Port<T, E>, slot: usize, waker: &mut Option<Waker>) -> Option<Waker> {
        if port.wakers[slot]
            .as_ref()
            .is_some_and(|previous| previous.will_wake(waker.as_ref().expect("new waker")))
        {
            return None;
        }
        std::mem::replace(&mut port.wakers[slot], waker.take())
    }
    /// Drops a waker while isolating any panic from its destructor.
    /// # Parameters
    /// - `waker` — optional waker to discard.
    fn discard_waker(waker: Option<Waker>) {
        if let Some(waker) = waker
            && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)))
        {
            dispose_panic_payload(payload);
        }
    }
}

impl<R: Send + 'static, T: Send + 'static, E: Send + 'static> PreparedLocalJob<R, T, E> {
    /// Runs the take request behavior while preserving the operation lifecycle
    /// invariants.
    pub(crate) fn take_request(&self) -> Option<R> {
        self.request.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
    /// Releases retained job resources and marks the operation as finished.
    /// # Parameters
    /// - `id` — operation identifier stored in the empty settlement report.
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
/// Creates an empty settlement report for a job released before execution.
/// # Parameters
/// - `id` — operation identifier associated with the released job.
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
    /// Records the first stop reason and wakes pollers waiting on job state.
    /// # Parameters
    /// - `reason` — stop reason to record if no reason has been recorded yet.
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
    /// Polls for the child outcome summary and registers the current waker.
    /// # Parameters
    /// - `cx` — task context whose waker is registered while the summary is pending.
    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<ChildOutcomeSummary> {
        if let Some(value) = self.port.lock().unwrap_or_else(|e| e.into_inner()).summary.clone() {
            return Poll::Ready(value);
        }
        let (mut new_waker, mut clone_panic) = match super::super::stream::clone_poll_waker(cx.waker()) {
            Ok(waker) => (Some(waker), None),
            Err(payload) => (None, Some(payload)),
        };
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = &port.summary {
            let value = value.clone();
            drop(port);
            Self::discard_waker(new_waker.take());
            if let Some(payload) = clone_panic.take() {
                dispose_panic_payload(payload);
            }
            return Poll::Ready(value);
        }
        if let Some(payload) = clone_panic.take() {
            drop(port);
            std::panic::resume_unwind(payload);
        }
        let old = Self::register(&mut port, 1, &mut new_waker);
        drop(port);
        Self::discard_waker(old);
        Self::discard_waker(new_waker);
        Poll::Pending
    }
    /// Polls for resource release and registers the current waker if pending.
    /// # Parameters
    /// - `cx` — task context whose waker is registered until release.
    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<ChildRelease> {
        let ready = {
            let port = self.port.lock().unwrap_or_else(|e| e.into_inner());
            port.released.then(|| ChildRelease {
                operation_id: port.id.expect("manager binds before observing control"),
                confirmed_at: None,
            })
        };
        if let Some(released) = ready {
            return Poll::Ready(released);
        }
        let (mut new_waker, mut clone_panic) = match super::super::stream::clone_poll_waker(cx.waker()) {
            Ok(waker) => (Some(waker), None),
            Err(payload) => (None, Some(payload)),
        };
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if port.released {
            let released = ChildRelease {
                operation_id: port.id.expect("manager binds before observing control"),
                confirmed_at: None,
            };
            drop(port);
            Self::discard_waker(new_waker.take());
            if let Some(payload) = clone_panic.take() {
                dispose_panic_payload(payload);
            }
            return Poll::Ready(released);
        }
        if let Some(payload) = clone_panic.take() {
            drop(port);
            std::panic::resume_unwind(payload);
        }
        let old = Self::register(&mut port, 2, &mut new_waker);
        drop(port);
        Self::discard_waker(old);
        Self::discard_waker(new_waker);
        Poll::Pending
    }
    /// Polls for the settlement report and registers the current waker.
    /// # Parameters
    /// - `cx` — task context whose waker is registered while settlement is pending.
    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        if let Some(value) = self.port.lock().unwrap_or_else(|e| e.into_inner()).settlement.clone() {
            return Poll::Ready(value);
        }
        let (mut new_waker, mut clone_panic) = match super::super::stream::clone_poll_waker(cx.waker()) {
            Ok(waker) => (Some(waker), None),
            Err(payload) => (None, Some(payload)),
        };
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = &port.settlement {
            let value = Arc::clone(value);
            drop(port);
            Self::discard_waker(new_waker.take());
            if let Some(payload) = clone_panic.take() {
                dispose_panic_payload(payload);
            }
            return Poll::Ready(value);
        }
        if let Some(payload) = clone_panic.take() {
            drop(port);
            std::panic::resume_unwind(payload);
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
    /// Binds this prepared job to its manager-assigned operation identifier.
    /// # Parameters
    /// - `id` — operation identifier assigned by the manager.
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
    /// Activates the prepared job by queueing its owned request and task.
    /// # Parameters
    /// - `scope` — admitted operation scope used by the worker task.
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
                let scope_stop = scope_guard.context().stop_reason();
                let stop = if let Some(reason) = port.stop {
                    Some(reason)
                } else if let Some(reason) = scope_stop {
                    port.stop = Some(reason);
                    Some(reason)
                } else {
                    port.phase = ExecutionPhase::Running;
                    None
                };
                (stop, Self::take_wakers(&mut port))
            };
            Self::wake_all(wakers);
            if let Some(reason) = stop_before_start {
                // Drop captured resources before returning their shared byte
                // reservation. Keep cleanup outside the result port lock.
                drop(task);
                drop(request);
                if let Some(permit) = this._owned_permit.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let mut permit = permit.lock().unwrap_or_else(|e| e.into_inner());
                    let amount = permit.amount();
                    permit.shrink(amount);
                }
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
                Err(payload) => {
                    dispose_panic_payload(payload);
                    OperationOutcome::ExecutionFailed(ExecutionFailure {
                        operation_id: id,
                        summary: DiagnosticSummary::new("local worker job panicked"),
                        source: None,
                    })
                }
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
    /// Polls for and takes the operation outcome, registering the waker if pending.
    /// # Parameters
    /// - `cx` — task context whose waker is registered until an outcome is ready.
    fn poll_take_outcome(&self, cx: &mut Context<'_>) -> Poll<OperationOutcome<T, E>> {
        let ready = self.port.lock().unwrap_or_else(|e| e.into_inner()).outcome.take();
        if let Some(value) = ready {
            self._owned_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
            return Poll::Ready(value);
        }
        let (mut new_waker, mut clone_panic) = match super::super::stream::clone_poll_waker(cx.waker()) {
            Ok(waker) => (Some(waker), None),
            Err(payload) => (None, Some(payload)),
        };
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = port.outcome.take() {
            drop(port);
            self._owned_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
            Self::discard_waker(new_waker.take());
            if let Some(payload) = clone_panic.take() {
                dispose_panic_payload(payload);
            }
            return Poll::Ready(value);
        }
        if let Some(payload) = clone_panic.take() {
            drop(port);
            std::panic::resume_unwind(payload);
        }
        let old = Self::register(&mut port, 0, &mut new_waker);
        drop(port);
        Self::discard_waker(old);
        Self::discard_waker(new_waker);
        Poll::Pending
    }
}
/// Maps an operation outcome to its bounded child summary.
/// # Type Parameters
/// - `T` — successful outcome value type.
/// - `E` — failed outcome value type.
/// # Parameters
/// - `id` — operation identifier included in the summary.
/// - `outcome` — operation outcome to classify.
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

    struct PanicOnDrop;

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("panic payload destructor panicked");
        }
    }

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
    fn test_register_preserves_a_cloned_waker_registration() {
        let target = Arc::new(CountWake(AtomicUsize::new(0)));
        let registered = Waker::from(Arc::clone(&target));
        let same_target = registered.clone();
        let mut port = empty_port();
        port.wakers[1] = Some(registered);
        let mut replacement = Some(same_target);

        let old = PreparedLocalJob::<(), (), ()>::register(&mut port, 1, &mut replacement);
        assert_eq!(old.is_none(), replacement.is_some());
        assert!(port.wakers[1].is_some(), "registered waker remains in its slot");
        PreparedLocalJob::<(), (), ()>::discard_waker(old);
        drop(replacement);
        assert_eq!(0, target.0.load(Ordering::SeqCst));
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
    fn test_wake_all_deduplicates_equivalent_wakers() {
        let target = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&target));
        let wakers = [Some(waker.clone()), None, Some(waker.clone()), None];

        PreparedLocalJob::<(), (), ()>::wake_all(wakers);

        assert_eq!(1, target.0.load(Ordering::SeqCst));
    }

    #[test]
    fn test_wake_all_continues_after_a_waker_panics() {
        struct PanicWake;
        impl Wake for PanicWake {
            fn wake(self: Arc<Self>) {
                std::panic::panic_any(PanicOnDrop);
            }

            fn wake_by_ref(self: &Arc<Self>) {
                std::panic::panic_any(PanicOnDrop);
            }
        }

        let target = Arc::new(CountWake(AtomicUsize::new(0)));
        let wakers = [
            Some(Waker::from(Arc::new(PanicWake))),
            Some(Waker::from(Arc::clone(&target))),
            None,
            None,
        ];

        PreparedLocalJob::<(), (), ()>::wake_all(wakers);

        assert_eq!(1, target.0.load(Ordering::SeqCst));
    }

    #[test]
    fn test_discard_waker_contains_a_panicking_payload_destructor() {
        struct DropPanics;

        impl Drop for DropPanics {
            fn drop(&mut self) {
                std::panic::panic_any(PanicOnDrop);
            }
        }

        impl Wake for DropPanics {
            fn wake(self: Arc<Self>) {
                panic!("discard-only waker must not be woken");
            }
        }

        PreparedLocalJob::<(), (), ()>::discard_waker(Some(Waker::from(Arc::new(DropPanics))));
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
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PreparedExternalOperation;

    use super::PreparedLocalJob;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalFactObserver;
    use crate::operation::LocalIoExecutor;

    #[test]
    fn test_activation_rejects_a_second_attempt_without_disturbing_original_job() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let prepared_slot = Arc::new(Mutex::new(None));
        let slot_for_first = Arc::clone(&prepared_slot);
        let mut first = manager
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
                PreparedExternalOperation::bind(&*job, scope.id()).expect("bind job identity");
                PreparedExternalOperation::activate(Arc::clone(&job), scope)
                    .expect("first activation queues the worker");
                *slot_for_first.lock().unwrap_or_else(|error| error.into_inner()) = Some(job);
                Ok::<_, ()>(())
            })
            .expect("first operation");
        assert!(matches!(
            first.wait_outcome().expect("first outcome"),
            OperationOutcome::Succeeded(())
        ));

        let job = prepared_slot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .expect("prepared job");
        let job_after_retry = Arc::clone(&job);
        let mut second = manager
            .try_submit(Default::default(), move |scope| {
                let failure = PreparedExternalOperation::activate(job, scope)
                    .expect_err("a prepared job accepts only one activation attempt");
                assert_eq!("local job activation was already attempted", failure.summary.as_str());
                Ok::<_, ()>(())
            })
            .expect("second operation");
        assert!(matches!(
            second.wait_outcome().expect("second outcome"),
            OperationOutcome::Succeeded(())
        ));
        let snapshot = qubit_operation::OperationControl::snapshot(&*job_after_retry);
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(qubit_operation::SettlementStatus::Released),
            snapshot.settlement.map(|report| report.status)
        );
    }

    #[test]
    fn test_activation_releases_job_when_scope_identity_does_not_match_binding() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let other_id = manager
            .try_submit(Default::default(), |_| Ok::<_, ()>(()))
            .expect("other operation")
            .id();
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
                PreparedExternalOperation::bind(&*job, other_id).expect("bind another identity");

                let failure = PreparedExternalOperation::activate(job.clone(), scope)
                    .expect_err("activation must reject a scope with a different identity");
                assert_eq!("local job identity mismatch", failure.summary.as_str());
                let snapshot = OperationControl::snapshot(&*job);
                assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
                assert!(snapshot.input_released);
                assert_eq!(
                    Some(qubit_operation::SettlementStatus::Released),
                    snapshot.settlement.map(|s| s.status)
                );
                assert!(job.task.lock().unwrap_or_else(|error| error.into_inner()).is_none());
                Ok::<_, ()>(())
            })
            .expect("parent operation");

        assert!(matches!(
            operation.wait_outcome().expect("parent outcome"),
            OperationOutcome::Succeeded(())
        ));
    }

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

    #[test]
    fn test_activation_releases_job_when_request_was_already_taken() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let observed = Arc::new(Mutex::new(None));
        let observed_job = Arc::clone(&observed);
        let mut operation = manager
            .try_submit(Default::default(), move |scope| {
                let job = PreparedLocalJob::new(
                    Arc::clone(&executor.inner),
                    String::from("one-shot request"),
                    executor.reserve_slot().expect("queue slot"),
                    Arc::new(Mutex::new(executor.reserve_bytes(0).expect("owned permit"))),
                    executor.reserve_handle().expect("handle permit"),
                    LocalFactObserver::new(),
                    |request| {
                        assert_eq!("one-shot request", request);
                        OperationOutcome::<usize, ()>::Succeeded(1)
                    },
                );
                assert_eq!(Some(String::from("one-shot request")), job.take_request());
                PreparedExternalOperation::bind(&*job, scope.id()).expect("bind job identity");
                let failure = PreparedExternalOperation::activate(Arc::clone(&job), scope)
                    .expect_err("activation must fail after the owned request was consumed");
                assert_eq!("local request already taken", failure.summary.as_str());
                *observed_job.lock().unwrap_or_else(|error| error.into_inner()) = Some(job);
                Ok::<_, ()>(())
            })
            .expect("parent operation");
        assert!(matches!(
            operation.wait_outcome().expect("parent outcome"),
            OperationOutcome::Succeeded(())
        ));

        let job = observed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .expect("prepared job");
        let snapshot = OperationControl::snapshot(&*job);
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(qubit_operation::SettlementStatus::Released),
            snapshot.settlement.map(|s| s.status)
        );
        assert!(job.task.lock().unwrap_or_else(|error| error.into_inner()).is_none());
    }

    #[test]
    fn test_activation_releases_job_when_worker_queue_is_stopped() {
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
                PreparedExternalOperation::bind(&*job, scope.id()).expect("bind job identity");
                executor.inner.queue.stop();

                let failure = PreparedExternalOperation::activate(Arc::clone(&job), scope)
                    .expect_err("activation must report a stopped worker queue");
                assert_eq!("local worker queue is full", failure.summary.as_str());
                let snapshot = OperationControl::snapshot(&*job);
                assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
                assert!(snapshot.input_released);
                assert_eq!(
                    Some(qubit_operation::SettlementStatus::Released),
                    snapshot.settlement.map(|settlement| settlement.status)
                );
                assert!(job.task.lock().unwrap_or_else(|error| error.into_inner()).is_none());
                Ok::<_, ()>(())
            })
            .expect("parent operation");

        assert!(matches!(
            operation.wait_outcome().expect("parent outcome"),
            OperationOutcome::Succeeded(())
        ));
    }

    #[test]
    fn test_activation_releases_job_when_worker_queue_is_full() {
        use std::sync::mpsc;
        use std::time::Duration;

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                ..LocalExecutionConfig::default()
            },
        )
        .expect("executor");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut release = Some(release_tx);
        executor
            .enqueue_task(move || {
                started_tx.send(()).expect("test receiver remains alive");
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("test releases the worker before its barrier expires");
            })
            .expect("worker barrier is accepted");
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker reaches the barrier");

        let queued_work = Arc::new(AtomicUsize::new(0));
        let queue_limit = executor.inner.config.queue_capacity + executor.inner.config.waiter_capacity;
        for _ in 0..queue_limit {
            let queued_work = Arc::clone(&queued_work);
            assert!(
                executor
                    .inner
                    .queue
                    .push(
                        Box::new(move || {
                            queued_work.fetch_add(1, Ordering::SeqCst);
                        }),
                        false,
                        queue_limit,
                    )
                    .is_ok(),
                "fill one ordinary queue slot"
            );
        }

        let observed = Arc::new(Mutex::new(None));
        let observed_job = Arc::clone(&observed);
        let executor_for_job = executor.clone();
        let queued_work_for_job = Arc::clone(&queued_work);
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                let job = PreparedLocalJob::<String, (), ()>::new(
                    Arc::clone(&executor_for_job.inner),
                    String::from("rejected request"),
                    executor_for_job.reserve_slot().expect("queue or waiter permit"),
                    Arc::new(Mutex::new(executor_for_job.reserve_bytes(7).expect("owned permit"))),
                    executor_for_job.reserve_handle().expect("handle permit"),
                    LocalFactObserver::new(),
                    |_| panic!("a rejected worker job must not run"),
                );
                PreparedExternalOperation::bind(&*job, scope.id()).expect("bind job identity");
                let failure = PreparedExternalOperation::activate(Arc::clone(&job), scope)
                    .expect_err("activation must reject a full worker queue");
                assert_eq!("local worker queue is full", failure.summary.as_str());

                let snapshot = OperationControl::snapshot(&*job);
                assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
                assert!(snapshot.input_released);
                assert_eq!(
                    Some(qubit_operation::SettlementStatus::Released),
                    snapshot.settlement.map(|s| s.status)
                );
                assert_eq!(None, job.take_request());
                assert!(job.task.lock().unwrap_or_else(|error| error.into_inner()).is_none());
                assert!(job._slot.lock().unwrap_or_else(|error| error.into_inner()).is_none());
                assert!(
                    job._owned_permit
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .is_none()
                );
                assert!(
                    job._handle_permit
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .is_none()
                );
                assert_eq!(0, executor_for_job.inner.queue_slots.used());
                assert_eq!(0, executor_for_job.inner.waiter_slots.used());
                assert_eq!(0, executor_for_job.inner.byte_slots.used());
                assert_eq!(0, executor_for_job.inner.handle_slots.used());
                assert_eq!(0, queued_work_for_job.load(Ordering::SeqCst));
                *observed_job.lock().unwrap_or_else(|error| error.into_inner()) = Some(job);
                Ok::<_, ()>(())
            })
            .expect("parent operation");
        assert!(matches!(
            parent.wait_outcome().expect("parent outcome"),
            OperationOutcome::Succeeded(())
        ));

        release
            .take()
            .expect("worker release sender")
            .send(())
            .expect("worker is still waiting");
        let drain_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while queued_work.load(Ordering::SeqCst) < queue_limit && std::time::Instant::now() < drain_deadline {
            std::thread::yield_now();
        }
        assert_eq!(
            queue_limit,
            queued_work.load(Ordering::SeqCst),
            "accepted queue entries drain"
        );
        let job = observed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .expect("rejected prepared job");
        assert!(
            OperationControl::poll_release(&*job, &mut std::task::Context::from_waker(std::task::Waker::noop()))
                .is_ready()
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ExecutionPhase;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PreparedExternalOperation;
    use qubit_operation::StopReason;

    use super::PreparedLocalJob;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalFactObserver;
    use crate::operation::LocalIoExecutor;

    struct PanicOnDrop;

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("panic payload destructor panicked");
        }
    }

    struct PanicPayloadWake;

    impl Wake for PanicPayloadWake {
        fn wake(self: Arc<Self>) {
            std::panic::panic_any(PanicOnDrop);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            std::panic::panic_any(PanicOnDrop);
        }
    }

    #[test]
    fn test_stop_before_worker_start_releases_and_publishes_all_results() {
        struct Notify(std::sync::mpsc::Sender<()>);
        impl Wake for Notify {
            fn wake(self: Arc<Self>) {
                let _ = self.0.send(());
            }

            fn wake_by_ref(self: &Arc<Self>) {
                let _ = self.0.send(());
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let executor_for_assertions = executor.clone();
        let observed = Arc::new(Mutex::new(None));
        let observed_job = Arc::clone(&observed);
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                let job = PreparedLocalJob::<(), (), ()>::new(
                    Arc::clone(&executor.inner),
                    (),
                    executor.reserve_slot().expect("queue slot"),
                    Arc::new(Mutex::new(executor.reserve_bytes(19).expect("owned permit"))),
                    executor.reserve_handle().expect("handle permit"),
                    LocalFactObserver::new(),
                    |_| panic!("a stop accepted before start must skip the worker"),
                );
                #[cfg(panic = "unwind")]
                job.poison_lifecycle_locks();
                job.port
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .wakers[0] = Some(Waker::from(Arc::new(PanicPayloadWake)));
                PreparedExternalOperation::bind(&*job, scope.id()).expect("bind job identity");
                *observed_job.lock().unwrap_or_else(|error| error.into_inner()) = Some(Arc::clone(&job));
                // Cancellation may already be accepted by the operation
                // scope while forwarding it to this local control is queued.
                assert!(scope.context().request_stop(StopReason::UserCancelled));
                assert!(!scope.context().request_stop(StopReason::Timeout));
                PreparedExternalOperation::activate(job, scope).expect("activation queues the stopped job");

                // Parent completion does not imply that its queued child has
                // been dequeued and settled. Wait on the child's lifecycle
                // signal before asserting its terminal snapshot below.
                let job = observed_job
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_ref()
                    .cloned()
                    .expect("prepared job");
                let (wake_tx, wake_rx) = std::sync::mpsc::channel();
                let waker = Waker::from(Arc::new(Notify(wake_tx)));
                let mut context = Context::from_waker(&waker);
                loop {
                    match OperationControl::poll_settlement(&*job, &mut context) {
                        Poll::Ready(_) => break,
                        Poll::Pending => wake_rx
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .expect("stopped prepared job must settle"),
                    }
                }
                Ok::<_, ()>(())
            })
            .expect("parent operation");
        assert!(matches!(
            parent.wait_outcome().expect("parent outcome"),
            OperationOutcome::Stopped(_)
        ));

        let job = observed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .expect("prepared job");
        let snapshot = OperationControl::snapshot(&*job);
        assert_eq!(ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(Some(StopReason::UserCancelled), snapshot.stop.map(|stop| stop.reason));
        assert_eq!(
            Some(qubit_operation::ChildOutcomeKind::Stopped),
            snapshot.outcome.map(|outcome| outcome.kind)
        );
        assert_eq!(0, executor_for_assertions.inner.byte_slots.used());
        let replacement = executor_for_assertions
            .reserve_bytes(19)
            .expect("stopped queued work returns its byte reservation before settlement");

        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(OperationControl::poll_outcome(&*job, &mut cx), Poll::Ready(_)));
        assert!(matches!(OperationControl::poll_release(&*job, &mut cx), Poll::Ready(_)));
        assert!(
            matches!(OperationControl::poll_settlement(&*job, &mut cx), Poll::Ready(report) if report.status == qubit_operation::SettlementStatus::Released)
        );
        assert!(matches!(
            PreparedExternalOperation::poll_take_outcome(&*job, &mut cx),
            Poll::Ready(OperationOutcome::Stopped(_))
        ));
        drop(replacement);
        assert_eq!(0, executor_for_assertions.inner.byte_slots.used());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Debug, PartialEq)]
    struct MatrixRequest(u16);

    #[derive(Debug, PartialEq)]
    struct MatrixResult(String);

    #[derive(Debug, PartialEq)]
    struct MatrixError(&'static str);

    #[derive(Debug, PartialEq)]
    struct AlternateRequest(Vec<u8>);

    #[derive(Debug, PartialEq)]
    struct AlternateResult(usize);

    #[derive(Debug, PartialEq)]
    struct AlternateError(u8);

    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PreparedExternalOperation;

    use super::PreparedLocalJob;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalFactObserver;
    use crate::operation::executor::LocalIoExecutor;

    fn prepare<T, E, F>(executor: &LocalIoExecutor, task: F) -> std::sync::Arc<PreparedLocalJob<(), T, E>>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(()) -> OperationOutcome<T, E> + Send + 'static,
    {
        PreparedLocalJob::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            executor.reserve_slot().unwrap(),
            std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(0).unwrap())),
            executor.reserve_handle().unwrap(),
            LocalFactObserver::new(),
            task,
        )
    }

    #[test]
    fn test_external_bind_is_one_shot_and_success_is_observable() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let prepared = prepare(&executor, |_| OperationOutcome::<u8, ()>::Succeeded(42));
        let mut handle = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&prepared))
            .unwrap();

        let id = OperationControl::snapshot(&*prepared).operation_id;
        assert!(matches!(
            PreparedExternalOperation::bind(&*prepared, id),
            Err(qubit_operation::ExternalBindError::AlreadyBound)
        ));
        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(42)
        ));
        assert!(OperationControl::snapshot(&*prepared).input_released);
    }

    #[test]
    fn test_non_unit_request_and_error_traverse_external_lifecycle() {
        use std::task::Context;
        use std::task::Poll;
        use std::task::Waker;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let prepared = PreparedLocalJob::new(
            std::sync::Arc::clone(&executor.inner),
            String::from("request payload"),
            executor.reserve_slot().unwrap(),
            std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(0).unwrap())),
            executor.reserve_handle().unwrap(),
            LocalFactObserver::new(),
            |request| {
                assert_eq!("request payload", request);
                OperationOutcome::<usize, std::io::Error>::Failed(std::io::Error::other("domain failure"))
            },
        );
        let mut handle = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&prepared))
            .unwrap();

        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Failed(error) if error.to_string() == "domain failure"
        ));
        let summary = OperationControl::snapshot(&*prepared)
            .outcome
            .expect("worker publishes an outcome summary");
        assert_eq!(qubit_operation::ChildOutcomeKind::Failed, summary.kind);

        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            OperationControl::poll_outcome(&*prepared, &mut context),
            Poll::Ready(summary) if summary.kind == qubit_operation::ChildOutcomeKind::Failed
        ));
        assert!(OperationControl::poll_release(&*prepared, &mut context).is_ready());
        assert!(OperationControl::poll_settlement(&*prepared, &mut context).is_ready());
    }

    #[test]
    fn test_external_lifecycle_matrix_covers_distinct_typed_jobs_and_release() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();

        let first = PreparedLocalJob::new(
            Arc::clone(&executor.inner),
            MatrixRequest(7),
            executor.reserve_slot().unwrap(),
            Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            executor.reserve_handle().unwrap(),
            LocalFactObserver::new(),
            |request| {
                assert_eq!(7, request.0);
                OperationOutcome::<MatrixResult, MatrixError>::Succeeded(MatrixResult("seven".into()))
            },
        );
        let mut first_handle = manager
            .try_submit_external(Default::default(), Arc::clone(&first))
            .unwrap();
        assert!(matches!(
            first_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(MatrixResult(value)) if value == "seven"
        ));

        let second = PreparedLocalJob::new(
            Arc::clone(&executor.inner),
            AlternateRequest(vec![2, 3]),
            executor.reserve_slot().unwrap(),
            Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            executor.reserve_handle().unwrap(),
            LocalFactObserver::new(),
            |request| {
                assert_eq!(vec![2, 3], request.0);
                OperationOutcome::<AlternateResult, AlternateError>::Failed(AlternateError(9))
            },
        );
        let mut second_handle = manager
            .try_submit_external(Default::default(), Arc::clone(&second))
            .unwrap();
        assert!(matches!(
            second_handle.wait_outcome().unwrap(),
            OperationOutcome::Failed(AlternateError(9))
        ));

        // A mismatched scope exercises the same typed job's release path
        // before its worker consumes the request.
        let other_id = manager
            .try_submit(Default::default(), |_| Ok::<_, ()>(()))
            .unwrap()
            .id();
        let mut mismatch = manager
            .try_submit(Default::default(), move |scope| {
                let job = PreparedLocalJob::new(
                    Arc::clone(&executor.inner),
                    AlternateRequest(vec![5]),
                    executor.reserve_slot().unwrap(),
                    Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
                    executor.reserve_handle().unwrap(),
                    LocalFactObserver::new(),
                    |_| OperationOutcome::<AlternateResult, AlternateError>::Succeeded(AlternateResult(1)),
                );
                PreparedExternalOperation::bind(&*job, other_id).unwrap();
                let failure = PreparedExternalOperation::activate(job.clone(), scope).unwrap_err();
                assert_eq!("local job identity mismatch", failure.summary.as_str());
                assert!(OperationControl::snapshot(&*job).input_released);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            mismatch.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
    }

    #[test]
    fn test_external_worker_failure_and_panic_become_typed_outcomes() {
        struct PanicOnDrop;

        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("panic payload destructor panicked");
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();

        let failed = prepare(&executor, |_| OperationOutcome::<(), _>::Failed("expected"));
        let mut failed_handle = manager.try_submit_external(Default::default(), failed).unwrap();
        assert!(matches!(
            failed_handle.wait_outcome().unwrap(),
            OperationOutcome::Failed("expected")
        ));

        let panicked = prepare::<(), (), _>(&executor, |_| std::panic::panic_any(PanicOnDrop));
        let mut panic_handle = manager.try_submit_external(Default::default(), panicked).unwrap();
        assert!(matches!(
            panic_handle.wait_outcome().unwrap(),
            OperationOutcome::ExecutionFailed(_)
        ));
    }

    #[test]
    fn test_registered_lifecycle_polls_become_ready_after_completion() {
        use std::sync::mpsc;
        use std::task::Context;
        use std::task::Wake;
        use std::task::Waker;

        struct Counter(AtomicUsize);
        impl Wake for Counter {
            fn wake(self: std::sync::Arc<Self>) {
                self.wake_by_ref();
            }

            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        struct ReleaseGuard(Option<mpsc::Sender<()>>);
        impl ReleaseGuard {
            fn release(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        impl Drop for ReleaseGuard {
            fn drop(&mut self) {
                self.release();
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut release = ReleaseGuard(Some(release_tx));
        let prepared = prepare(&executor, move |_| {
            started_tx.send(()).unwrap();
            release_rx
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("test must release the worker before its barrier expires");
            OperationOutcome::<(), ()>::Succeeded(())
        });
        let mut handle = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&prepared))
            .unwrap();
        started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();

        let counter = std::sync::Arc::new(Counter(AtomicUsize::new(0)));
        let waker = Waker::from(std::sync::Arc::clone(&counter));
        let mut context = Context::from_waker(&waker);
        assert!(OperationControl::poll_outcome(&*prepared, &mut context).is_pending());
        assert!(OperationControl::poll_release(&*prepared, &mut context).is_pending());
        assert!(OperationControl::poll_settlement(&*prepared, &mut context).is_pending());

        // A stop request wakes each registered control poll while the worker
        // remains held at the channel barrier.
        OperationControl::request_stop(&*prepared, qubit_operation::StopReason::UserCancelled);
        assert!(counter.0.load(Ordering::SeqCst) > 0);
        release.release();
        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(OperationControl::poll_outcome(&*prepared, &mut context).is_ready());
        assert!(OperationControl::poll_release(&*prepared, &mut context).is_ready());
        assert!(OperationControl::poll_settlement(&*prepared, &mut context).is_ready());
    }
}

#[cfg(test)]
mod prepared_job_ownership_tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PreparedExternalOperation;

    use super::PreparedLocalJob;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalFactObserver;
    use crate::operation::LocalIoExecutor;

    struct WakeCount(AtomicUsize);

    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn make_job(executor: &LocalIoExecutor) -> Arc<PreparedLocalJob<String, (), ()>> {
        PreparedLocalJob::new(
            Arc::clone(&executor.inner),
            String::from("owned request"),
            executor.reserve_slot().expect("queue slot"),
            Arc::new(Mutex::new(executor.reserve_bytes(0).expect("owned permit"))),
            executor.reserve_handle().expect("handle permit"),
            LocalFactObserver::new(),
            |_| OperationOutcome::<(), ()>::Succeeded(()),
        )
    }

    #[test]
    fn test_ready_lifecycle_polls_do_not_clone_hostile_waker() {
        use std::task::Context;
        use std::task::Poll;

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let job = make_job(&executor);
        let id = operation_id(&manager);
        PreparedExternalOperation::bind(&*job, id).expect("bind identity");
        let outcome = OperationOutcome::<(), ()>::Succeeded(());
        {
            let mut port = job.port.lock().unwrap();
            port.summary = Some(super::outcome_summary(id, &outcome));
            port.outcome = Some(outcome);
            port.released = true;
            port.settlement = Some(super::empty_settlement(id));
        }

        let hostile = crate::operation::stream::panic_clone_waker();
        let mut context = Context::from_waker(&hostile);
        assert!(matches!(
            OperationControl::poll_outcome(&*job, &mut context),
            Poll::Ready(summary) if summary.operation_id == id
        ));
        assert!(matches!(
            OperationControl::poll_release(&*job, &mut context),
            Poll::Ready(release) if release.operation_id == id
        ));
        assert!(matches!(
            OperationControl::poll_settlement(&*job, &mut context),
            Poll::Ready(report) if report.operation_id == id
        ));
        assert!(matches!(
            PreparedExternalOperation::poll_take_outcome(&*job, &mut context),
            Poll::Ready(OperationOutcome::Succeeded(()))
        ));
        drop(job);
        let shutdown = executor
            .request_shutdown(qubit_operation::ShutdownMode::Drain)
            .wait()
            .expect("executor shuts down");
        assert_eq!(0, shutdown.snapshot.active_operations);
    }

    fn operation_id(manager: &OperationManager) -> qubit_operation::OperationId {
        manager
            .try_submit(Default::default(), |_| Ok::<(), ()>(()))
            .expect("operation admitted")
            .id()
    }

    #[test]
    fn test_prepared_request_is_transferred_at_most_once() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).expect("executor");
        let job = make_job(&executor);

        assert_eq!(Some(String::from("owned request")), job.take_request());
        assert_eq!(None, job.take_request());
    }

    #[test]
    fn test_finish_released_clears_prepared_state_and_publishes_settlement() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let job = make_job(&executor);
        let id = operation_id(&manager);
        PreparedExternalOperation::bind(&*job, id).expect("bind job identity");
        let wakes: Vec<_> = (0..4).map(|_| Arc::new(WakeCount(AtomicUsize::new(0)))).collect();
        {
            let mut port = job.port.lock().unwrap_or_else(|error| error.into_inner());
            port.wakers = std::array::from_fn(|index| Some(Waker::from(Arc::clone(&wakes[index]))));
        }

        #[cfg(panic = "unwind")]
        job.poison_lifecycle_locks();
        job.finish_released(id);

        let snapshot = OperationControl::snapshot(&*job);
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(qubit_operation::SettlementStatus::Released),
            snapshot.settlement.map(|s| s.status)
        );
        assert_eq!(None, job.take_request());
        assert!(job.task.lock().unwrap_or_else(|error| error.into_inner()).is_none());
        assert!(job._slot.lock().unwrap_or_else(|error| error.into_inner()).is_none());
        assert!(
            job._owned_permit
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
        );
        assert!(
            job._handle_permit
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
        );
        assert!(
            job._fact_observer
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
        );
        assert!(wakes.iter().all(|wake| wake.0.load(Ordering::SeqCst) == 1));
    }

    #[test]
    fn test_abandon_only_publishes_terminal_state_after_binding() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");

        let unbound = make_job(&executor);
        PreparedExternalOperation::abandon(Arc::clone(&unbound));
        {
            let port = unbound.port.lock().unwrap_or_else(|error| error.into_inner());
            assert_eq!(qubit_operation::ExecutionPhase::Queued, port.phase);
            assert!(!port.released);
            assert!(port.settlement.is_none());
        }

        let bound = make_job(&executor);
        let id = operation_id(&manager);
        PreparedExternalOperation::bind(&*bound, id).expect("bind job identity");
        PreparedExternalOperation::abandon(Arc::clone(&bound));
        let snapshot = OperationControl::snapshot(&*bound);
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(qubit_operation::SettlementStatus::Released),
            snapshot.settlement.map(|s| s.status)
        );
        assert_eq!(None, bound.take_request());
    }

    #[test]
    fn test_abandon_wakes_registered_lifecycle_polls_and_publishes_ready_results() {
        use std::task::Context;
        use std::task::Poll;

        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let job = make_job(&executor);
        let id = operation_id(&manager);
        #[cfg(panic = "unwind")]
        job.poison_lifecycle_locks();
        PreparedExternalOperation::bind(&*job, id).expect("bind job identity");
        let wake = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        let mut context = Context::from_waker(&waker);

        assert!(OperationControl::poll_outcome(&*job, &mut context).is_pending());
        assert!(OperationControl::poll_release(&*job, &mut context).is_pending());
        assert!(OperationControl::poll_settlement(&*job, &mut context).is_pending());

        PreparedExternalOperation::abandon(Arc::clone(&job));

        // All three control polls share one task waker, so wake_all
        // deduplicates it.
        assert_eq!(1, wake.0.load(Ordering::SeqCst));
        assert!(matches!(
            OperationControl::poll_outcome(&*job, &mut context),
            Poll::Pending
        ));
        assert!(matches!(
            OperationControl::poll_release(&*job, &mut context),
            Poll::Ready(release) if release.operation_id == id
        ));
        assert!(matches!(
            OperationControl::poll_settlement(&*job, &mut context),
            Poll::Ready(report) if report.operation_id == id && report.status == qubit_operation::SettlementStatus::Released
        ));
        let snapshot = OperationControl::snapshot(&*job);
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert!(snapshot.outcome.is_none());
    }

    #[test]
    fn test_activation_after_request_transfer_fails_and_releases_job() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                let job = make_job(&executor);
                PreparedExternalOperation::bind(&*job, scope.id()).expect("bind job identity");

                assert_eq!(Some(String::from("owned request")), job.take_request());
                let failure = PreparedExternalOperation::activate(Arc::clone(&job), scope)
                    .expect_err("activation cannot proceed after the request was transferred");
                assert_eq!("local request already taken", failure.summary.as_str());

                let snapshot = OperationControl::snapshot(&*job);
                assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
                assert!(snapshot.input_released);
                assert_eq!(
                    Some(qubit_operation::SettlementStatus::Released),
                    snapshot.settlement.map(|report| report.status)
                );
                assert_eq!(None, job.take_request());
                Ok::<_, ()>(())
            })
            .expect("parent operation");

        assert!(matches!(
            parent.wait_outcome().expect("parent outcome"),
            OperationOutcome::Succeeded(())
        ));
    }
}

#[cfg(test)]
mod scoped_job_tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Waker;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PreparedExternalOperation;

    use super::PreparedLocalJob;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalFactObserver;
    use crate::operation::LocalIoExecutor;

    #[test]
    fn test_scoped_worker_receives_bound_identity_and_manager_consumes_result_once() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let observed_id = Arc::new(Mutex::new(None));
        let worker_id = Arc::clone(&observed_id);
        let job = PreparedLocalJob::new_scoped(
            Arc::clone(&executor.inner),
            String::from("scoped request"),
            executor.reserve_slot().expect("queue slot"),
            Arc::new(Mutex::new(executor.reserve_bytes(0).expect("owned permit"))),
            executor.reserve_handle().expect("handle permit"),
            LocalFactObserver::new(),
            move |request, scope| {
                assert_eq!("scoped request", request);
                *worker_id.lock().unwrap_or_else(|error| error.into_inner()) = Some(scope.id());
                OperationOutcome::<usize, &'static str>::Succeeded(37)
            },
        );
        #[cfg(panic = "unwind")]
        job.poison_lifecycle_locks();
        let mut handle = manager
            .try_submit_external(Default::default(), Arc::clone(&job))
            .expect("external operation admitted");
        let id = OperationControl::snapshot(&*job).operation_id;
        let outcome = handle.wait_outcome().expect("scoped worker outcome");
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            OperationControl::poll_outcome(&*job, &mut context),
            Poll::Ready(summary) if summary.operation_id == id
                && summary.kind == qubit_operation::ChildOutcomeKind::Succeeded
        ));
        assert!(matches!(
            OperationControl::poll_release(&*job, &mut context),
            Poll::Ready(release) if release.operation_id == id
        ));
        assert!(matches!(
            OperationControl::poll_settlement(&*job, &mut context),
            Poll::Ready(report) if report.operation_id == id
                && report.status == qubit_operation::SettlementStatus::Released
        ));
        assert_eq!(Some(id), *observed_id.lock().unwrap_or_else(|error| error.into_inner()));
        assert!(matches!(outcome, OperationOutcome::Succeeded(37)));
        assert!(matches!(
            PreparedExternalOperation::poll_take_outcome(&*job, &mut context),
            Poll::Pending
        ));
        drop(handle);
    }
}

#[cfg(all(test, panic = "unwind"))]
impl<R, T, E> PreparedLocalJob<R, T, E> {
    fn poison_lifecycle_locks(&self) {
        use crate::operation::poison_test_mutex;

        poison_test_mutex(&self.task);
        poison_test_mutex(&self.request);
        poison_test_mutex(&self._slot);
        poison_test_mutex(&self._owned_permit);
        poison_test_mutex(&self._handle_permit);
        poison_test_mutex(&self._fact_observer);
        poison_test_mutex(&self.port);
    }
}
