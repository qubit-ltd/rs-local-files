use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

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

use super::super::LocalFactObserver;
use super::super::executor::ExecutorInner;
use super::permits::Permit;

struct Port<T, E> {
    id: Option<OperationId>,
    phase: ExecutionPhase,
    stop: Option<StopReason>,
    outcome: Option<OperationOutcome<T, E>>,
    summary: Option<ChildOutcomeSummary>,
    released: bool,
    settlement: Option<Arc<SettlementReport>>,
    wakers: [Option<Waker>; 4],
}

/// Owned job prepared before manager admission. Binding and activation are one-shot.
pub(crate) struct PreparedLocalJob<R, T, E> {
    executor: Arc<ExecutorInner>,
    request: Mutex<Option<R>>,
    task: Mutex<Option<Box<dyn FnOnce(R) -> OperationOutcome<T, E> + Send + 'static>>>,
    _slot: Mutex<Option<Permit>>,
    _owned_permit: Mutex<Option<Arc<Mutex<Permit>>>>,
    _fact_observer: LocalFactObserver,
    _handle_permit: Mutex<Option<Permit>>,
    claimed: AtomicBool,
    port: Mutex<Port<T, E>>,
}

impl<R, T, E> PreparedLocalJob<R, T, E>
where
    R: Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
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
        Arc::new(Self {
            executor,
            request: Mutex::new(Some(request)),
            task: Mutex::new(Some(Box::new(task))),
            _slot: Mutex::new(Some(slot)),
            _owned_permit: Mutex::new(Some(owned_permit)),
            _fact_observer: fact_observer,
            _handle_permit: Mutex::new(Some(handle_permit)),
            claimed: AtomicBool::new(false),
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

    fn wake_all(port: &mut Port<T, E>) {
        let wakers = std::mem::replace(&mut port.wakers, [None, None, None, None]);
        for (index, waker) in wakers.iter().enumerate() {
            let Some(waker) = waker else {
                continue;
            };
            let duplicate = wakers[..index]
                .iter()
                .flatten()
                .any(|prior| prior.will_wake(waker));
            if !duplicate {
                waker.wake_by_ref();
            }
        }
    }

    fn register(port: &mut Port<T, E>, slot: usize, waker: &Waker) {
        port.wakers[slot] = Some(waker.clone());
    }
}

impl<R: Send + 'static, T: Send + 'static, E: Send + 'static> PreparedLocalJob<R, T, E> {
    pub(crate) fn take_request(&self) -> Option<R> {
        self.request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    fn finish_released(&self, id: OperationId) {
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        let outcome = OperationOutcome::ExecutionFailed(ExecutionFailure {
            operation_id: id,
            summary: DiagnosticSummary::new("local worker activation failed"),
            source: None,
        });
        port.phase = ExecutionPhase::Finished;
        port.summary = Some(outcome_summary(id, &outcome));
        port.outcome = Some(outcome);
        port.released = true;
        port.settlement = Some(empty_settlement(id));
        Self::wake_all(&mut port);
    }
}

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
    fn request_stop(&self, reason: StopReason) {
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if port.stop.is_none() {
            port.stop = Some(reason);
        }
        Self::wake_all(&mut port);
    }

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

    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<ChildOutcomeSummary> {
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = &port.summary {
            return Poll::Ready(value.clone());
        }
        Self::register(&mut port, 1, cx.waker());
        Poll::Pending
    }

    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<ChildRelease> {
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if port.released {
            return Poll::Ready(ChildRelease {
                operation_id: port.id.expect("manager binds before observing control"),
                confirmed_at: None,
            });
        }
        Self::register(&mut port, 2, cx.waker());
        Poll::Pending
    }

    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = &port.settlement {
            return Poll::Ready(Arc::clone(value));
        }
        Self::register(&mut port, 3, cx.waker());
        Poll::Pending
    }
}

impl<R, T, E> PreparedExternalOperation<T, E> for PreparedLocalJob<R, T, E>
where
    R: Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
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

    fn activate(self: Arc<Self>, scope: OperationScope) -> Result<(), ExecutionFailure> {
        let id = scope.id();
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
        let request = self
            .request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
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
            let _scope_guard = scope_guard;
            this._slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            let stop_before_start = {
                let mut port = this.port.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(reason) = port.stop {
                    Some(reason)
                } else {
                    port.phase = ExecutionPhase::Running;
                    Self::wake_all(&mut port);
                    None
                }
            };
            if let Some(reason) = stop_before_start {
                let outcome = OperationOutcome::Stopped(qubit_operation::StopReport {
                    reason,
                    accepted_at: None,
                });
                let mut port = this.port.lock().unwrap_or_else(|e| e.into_inner());
                port.phase = ExecutionPhase::Finished;
                port.summary = Some(outcome_summary(id, &outcome));
                port.outcome = Some(outcome);
                port.released = true;
                port.settlement = Some(empty_settlement(id));
                this._handle_permit
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
                Self::wake_all(&mut port);
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(request)));
            this._handle_permit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
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
            let mut port = this.port.lock().unwrap_or_else(|e| e.into_inner());
            port.phase = ExecutionPhase::Finished;
            port.summary = Some(summary);
            port.outcome = Some(outcome);
            port.released = true;
            port.settlement = Some(settlement);
            Self::wake_all(&mut port);
        });
        if let Err(_job) = self.executor.queue.push(
            job,
            false,
            self.executor
                .config
                .queue_capacity
                .saturating_add(self.executor.config.waiter_capacity),
        ) {
            self.finish_released(id);
            return Err(ExecutionFailure {
                operation_id: id,
                summary: DiagnosticSummary::new("local worker queue is full"),
                source: None,
            });
        }
        Ok(())
    }

    fn abandon(self: Arc<Self>) {
        self.task.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self._slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        self._owned_permit
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(id) = port.id {
            port.phase = ExecutionPhase::Finished;
            port.released = true;
            port.settlement = Some(empty_settlement(id));
        }
        Self::wake_all(&mut port);
    }

    fn poll_take_outcome(&self, cx: &mut Context<'_>) -> Poll<OperationOutcome<T, E>> {
        let mut port = self.port.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = port.outcome.take() {
            self._owned_permit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            return Poll::Ready(value);
        }
        Self::register(&mut port, 0, cx.waker());
        Poll::Pending
    }
}

fn outcome_summary<T, E>(id: OperationId, outcome: &OperationOutcome<T, E>) -> ChildOutcomeSummary {
    let (kind, text) = match outcome {
        OperationOutcome::Succeeded(_) => (ChildOutcomeKind::Succeeded, "child succeeded"),
        OperationOutcome::Failed(_) | OperationOutcome::ChildFailed(_) => {
            (ChildOutcomeKind::Failed, "child failed")
        }
        OperationOutcome::Stopped(_) => (ChildOutcomeKind::Stopped, "child stopped"),
        OperationOutcome::Panicked(panic) => (ChildOutcomeKind::Panicked, panic.summary.as_str()),
        OperationOutcome::ExecutionFailed(error) => {
            (ChildOutcomeKind::ExecutionFailed, error.summary.as_str())
        }
    };
    ChildOutcomeSummary {
        operation_id: id,
        kind,
        summary: DiagnosticSummary::new(text),
    }
}
