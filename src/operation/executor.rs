use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::thread::JoinHandle;

use qubit_operation::GroupOptions;
use qubit_operation::OperationGroup;
use qubit_operation::OperationManager;

use super::LocalExecutionConfig;
use super::LocalExecutorConfigError;
use super::LocalExecutorCreateError;
use super::internal::permits::DiskPermit;
use super::internal::permits::DiskPermitPool;
use super::internal::permits::Permit;
use super::internal::permits::PermitPool;
use super::internal::worker::WorkerQueue;

/// Bounded local I/O worker group associated with an application manager.
#[derive(Clone)]
pub struct LocalIoExecutor {
    pub(crate) inner: Arc<ExecutorInner>,
    _owner: Arc<WorkerOwner>,
}

pub(crate) struct ExecutorInner {
    pub(crate) group: OperationGroup,
    pub(crate) config: LocalExecutionConfig,
    pub(crate) queue: Arc<WorkerQueue>,
    pub(crate) queue_slots: PermitPool,
    pub(crate) waiter_slots: PermitPool,
    pub(crate) byte_slots: PermitPool,
    pub(crate) handle_slots: PermitPool,
    #[allow(dead_code)]
    pub(crate) disk_slots: DiskPermitPool,
    pub(crate) fact_slots: PermitPool,
    #[allow(dead_code)]
    pub(crate) cleanup_slots: PermitPool,
}

struct WorkerOwner {
    queue: Arc<WorkerQueue>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl LocalIoExecutor {
    /// Starts a bounded worker group attached to `manager`.
    pub fn new(
        manager: OperationManager,
        config: LocalExecutionConfig,
    ) -> Result<Self, LocalExecutorCreateError> {
        validate(&config).map_err(LocalExecutorCreateError::InvalidConfig)?;
        let group_capacity = config
            .queue_capacity
            .saturating_add(config.waiter_capacity)
            .saturating_add(config.worker_count)
            .max(1);
        let group = manager
            .group(GroupOptions {
                capacity: group_capacity,
            })
            .map_err(|error| LocalExecutorCreateError::WorkerStart(std::io::Error::other(error)))?;
        let queue = Arc::new(WorkerQueue::default());
        let workers = spawn_workers(&queue, config.worker_count, |index, worker_queue| {
            thread::Builder::new()
                .name(format!("qubit-local-io-{index}"))
                .spawn(move || worker_queue.run())
        })
        .map_err(LocalExecutorCreateError::WorkerStart)?;
        let owner = Arc::new(WorkerOwner {
            queue: Arc::clone(&queue),
            workers: Mutex::new(workers),
        });
        Ok(Self {
            inner: Arc::new(ExecutorInner {
                group,
                config: config.clone(),
                queue,
                queue_slots: PermitPool::new(config.queue_capacity),
                waiter_slots: PermitPool::new(config.waiter_capacity),
                byte_slots: PermitPool::new(config.max_owned_bytes),
                handle_slots: PermitPool::new(config.max_open_handles),
                disk_slots: DiskPermitPool::new(config.max_disk_bytes),
                fact_slots: PermitPool::new(config.max_fact_records),
                cleanup_slots: PermitPool::new(config.max_open_handles),
            }),
            _owner: owner,
        })
    }

    /// Returns the immutable resource and queue limits used by this executor.
    pub fn config(&self) -> &LocalExecutionConfig {
        &self.inner.config
    }

    pub(crate) fn reserve_slot(&self) -> Option<Permit> {
        self.inner
            .queue_slots
            .try_acquire(1)
            .or_else(|| self.inner.waiter_slots.try_acquire(1))
    }

    pub(crate) fn reserve_bytes(&self, amount: usize) -> Option<Permit> {
        self.inner.byte_slots.try_acquire(amount)
    }

    pub(crate) fn reserve_handle(&self) -> Option<Permit> {
        self.inner.handle_slots.try_acquire(1)
    }

    #[allow(dead_code)]
    pub(crate) fn reserve_disk(&self, amount: u64) -> Option<DiskPermit> {
        self.inner.disk_slots.try_acquire(amount)
    }

    pub(crate) fn reserve_fact(&self) -> Option<Permit> {
        self.inner.fact_slots.try_acquire(1)
    }

    pub(crate) fn enqueue_task<F>(&self, task: F) -> Result<(), ()>
    where
        F: FnOnce() + Send + 'static,
    {
        let Some(slot) = self.reserve_slot() else {
            return Err(());
        };
        let job = Box::new(move || {
            drop(slot);
            task();
        });
        match self.inner.queue.push(
            job,
            false,
            self.inner
                .config
                .queue_capacity
                .saturating_add(self.inner.config.waiter_capacity),
        ) {
            Ok(()) => Ok(()),
            Err(_) => Err(()),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn reserve_cleanup(&self) -> Option<Permit> {
        self.inner.cleanup_slots.try_acquire(1)
    }

    #[allow(dead_code)]
    pub(crate) fn enqueue_cleanup(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        let wrapped = Box::new(move || {
            job();
            drop(permit);
        });
        self.inner
            .queue
            .push(wrapped, true, self.inner.config.max_open_handles)
    }
}

fn spawn_workers<F>(
    queue: &Arc<WorkerQueue>,
    count: usize,
    mut spawn: F,
) -> io::Result<Vec<JoinHandle<()>>>
where
    F: FnMut(usize, Arc<WorkerQueue>) -> io::Result<JoinHandle<()>>,
{
    let mut workers = Vec::with_capacity(count);
    for index in 0..count {
        match spawn(index, Arc::clone(queue)) {
            Ok(handle) => workers.push(handle),
            Err(error) => {
                queue.stop();
                for handle in workers {
                    let _ = handle.join();
                }
                return Err(error);
            }
        }
    }
    Ok(workers)
}

impl Drop for WorkerOwner {
    fn drop(&mut self) {
        self.queue.stop();
        let workers = self.workers.get_mut().unwrap_or_else(|e| e.into_inner());
        for worker in workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn validate(config: &LocalExecutionConfig) -> Result<(), LocalExecutorConfigError> {
    if config.worker_count == 0 {
        return Err(LocalExecutorConfigError::ZeroWorkers);
    }
    if config.queue_capacity == 0 {
        return Err(LocalExecutorConfigError::ZeroQueueCapacity);
    }
    if config.waiter_capacity == 0 {
        return Err(LocalExecutorConfigError::ZeroWaiterCapacity);
    }
    if config.max_owned_bytes == 0 {
        return Err(LocalExecutorConfigError::ZeroOwnedBytes);
    }
    if config.max_open_handles == 0 {
        return Err(LocalExecutorConfigError::ZeroOpenHandles);
    }
    if config.max_disk_bytes == 0 {
        return Err(LocalExecutorConfigError::ZeroDiskBytes);
    }
    if config.max_fact_records == 0 {
        return Err(LocalExecutorConfigError::ZeroFactRecords);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::LocalExecutionConfig;
    use super::LocalIoExecutor;
    use super::spawn_workers;
    use crate::operation::AsyncLocalFileSystem;
    use crate::operation::LocalFactObserver;
    use crate::operation::internal::prepared_local_job::PreparedLocalJob;
    use crate::operation::internal::worker::WorkerQueue;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    fn make_job<F>(
        executor: &LocalIoExecutor,
        task: F,
    ) -> std::sync::Arc<PreparedLocalJob<(), (), ()>>
    where
        F: FnOnce(()) -> OperationOutcome<(), ()> + Send + 'static,
    {
        let slot = executor.reserve_slot().unwrap();
        make_job_with_slot(executor, slot, task)
    }

    fn make_job_with_slot<F>(
        executor: &LocalIoExecutor,
        slot: super::Permit,
        task: F,
    ) -> std::sync::Arc<PreparedLocalJob<(), (), ()>>
    where
        F: FnOnce(()) -> OperationOutcome<(), ()> + Send + 'static,
    {
        PreparedLocalJob::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            slot,
            std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(0).unwrap())),
            executor.reserve_handle().unwrap(),
            LocalFactObserver::new(),
            task,
        )
    }

    #[test]
    fn partial_worker_start_failure_stops_and_joins_started_workers() {
        let queue = std::sync::Arc::new(WorkerQueue::default());
        let (joined_tx, joined_rx) = mpsc::channel();
        let mut calls = 0;
        let result = spawn_workers(&queue, 3, |_, worker_queue| {
            calls += 1;
            if calls == 1 {
                let joined_tx = joined_tx.clone();
                std::thread::Builder::new().spawn(move || {
                    worker_queue.run();
                    joined_tx.send(()).unwrap();
                })
            } else {
                Err(io::Error::other("injected thread start failure"))
            }
        });

        assert!(result.is_err());
        assert_eq!(calls, 2);
        joined_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("started worker must exit before start failure is returned");
    }

    #[test]
    fn activate_queue_failure_publishes_terminal_outcome_and_release() {
        use std::task::{Context, Wake, Waker};

        struct Noop;
        impl Wake for Noop {
            fn wake(self: std::sync::Arc<Self>) {}
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor =
            LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let prepared = make_job(&executor, |_| OperationOutcome::Succeeded(()));
        let executor_for_parent = executor.clone();
        let prepared_for_parent = std::sync::Arc::clone(&prepared);
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                qubit_operation::PreparedExternalOperation::bind(&*prepared_for_parent, scope.id())
                    .unwrap();
                executor_for_parent.inner.queue.stop();
                assert!(
                    qubit_operation::PreparedExternalOperation::activate(
                        std::sync::Arc::clone(&prepared_for_parent),
                        scope,
                    )
                    .is_err()
                );

                let waker = Waker::from(std::sync::Arc::new(Noop));
                let mut context = Context::from_waker(&waker);
                assert!(matches!(
                    qubit_operation::PreparedExternalOperation::poll_take_outcome(
                        &*prepared_for_parent,
                        &mut context,
                    ),
                    std::task::Poll::Ready(OperationOutcome::ExecutionFailed(_))
                ));
                assert!(
                    qubit_operation::OperationControl::poll_release(
                        &*prepared_for_parent,
                        &mut context,
                    )
                    .is_ready()
                );
                assert!(
                    qubit_operation::OperationControl::poll_settlement(
                        &*prepared_for_parent,
                        &mut context,
                    )
                    .is_ready()
                );
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            parent.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
    }

    #[test]
    fn local_operation_keeps_result_budget_until_user_takes_outcome() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor =
            LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let permit = std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(8).unwrap()));
        let handle = manager
            .try_submit(Default::default(), |_| Ok::<_, ()>(vec![1_u8; 4]))
            .unwrap();
        let mut local = crate::operation::LocalOperation::new(handle, permit.clone());
        drop(permit);
        local.wait_settlement().unwrap();
        assert_eq!(executor.inner.byte_slots.used(), 8);
        let outcome = local.wait_outcome().unwrap();
        assert!(matches!(outcome, OperationOutcome::Succeeded(bytes) if bytes == [1, 1, 1, 1]));
        assert_eq!(executor.inner.byte_slots.used(), 0);
    }

    #[test]
    fn small_read_budgets_actual_capacity_until_take_or_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("small");
        std::fs::write(&path, b"tiny").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_owned_bytes: 64,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let filesystem = crate::LocalFileSystem::host()
            .unwrap()
            .into_async(executor.clone())
            .unwrap();
        let (mut operation, observer) = filesystem
            .submit_read_all_with_observer(
                &manager,
                Default::default(),
                crate::operation::LocalReadRequest {
                    path: path.clone(),
                    read: Default::default(),
                },
                Default::default(),
            )
            .unwrap();
        operation.wait_settlement().unwrap();
        let used_before_take = executor.inner.byte_slots.used();
        assert!(
            used_before_take > 0 && used_before_take < 64,
            "small result must not reserve the whole executor pool"
        );
        assert_eq!(executor.inner.fact_slots.used(), 1);
        let outcome = operation.wait_outcome().unwrap();
        let actual_capacity = match outcome {
            OperationOutcome::Succeeded(bytes) => bytes.capacity(),
            other => panic!("unexpected read outcome: {other:?}"),
        };
        assert_eq!(used_before_take, actual_capacity);
        assert_eq!(executor.inner.byte_slots.used(), 0);
        drop(observer);
        assert_eq!(executor.inner.fact_slots.used(), 0);

        let (operation, observer) = filesystem
            .submit_read_all_with_observer(
                &manager,
                Default::default(),
                crate::operation::LocalReadRequest {
                    path,
                    read: Default::default(),
                },
                Default::default(),
            )
            .unwrap();
        operation.wait_settlement().unwrap();
        assert!(executor.inner.byte_slots.used() > 0);
        drop(operation);
        assert_eq!(
            executor.inner.byte_slots.used(),
            0,
            "drop discards the unclaimed upstream result before returning its budget"
        );
        assert_eq!(executor.inner.fact_slots.used(), 1);
        drop(observer);
        assert_eq!(executor.inner.fact_slots.used(), 0);
    }

    #[test]
    fn async_constructor_poll_never_waits_for_blocked_worker_io() {
        use std::future::Future;
        use std::task::{Context, Poll, Wake, Waker};
        struct Noop;
        impl Wake for Noop {
            fn wake(self: std::sync::Arc<Self>) {}
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        let mut rooted = Box::pin(AsyncLocalFileSystem::rooted(executor.clone(), root));
        let waker = Waker::from(std::sync::Arc::new(Noop));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(rooted.as_mut().poll(&mut context), Poll::Pending));
        drop(rooted);
        let mut host = Box::pin(AsyncLocalFileSystem::host(executor.clone()));
        assert!(matches!(host.as_mut().poll(&mut context), Poll::Pending));
        drop(host);

        let (complete_tx, complete_rx) = mpsc::channel();
        executor
            .enqueue_task(move || complete_tx.send(()).unwrap())
            .unwrap();
        release_tx.send(()).unwrap();
        complete_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn cleanup_and_ordinary_queues_make_round_robin_progress() {
        let queue = std::sync::Arc::new(WorkerQueue::default());
        let worker_queue = std::sync::Arc::clone(&queue);
        let worker = std::thread::spawn(move || worker_queue.run());
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (order_tx, order_rx) = mpsc::channel();
        assert!(
            queue
                .push(
                    Box::new(move || {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }),
                    true,
                    4,
                )
                .is_ok()
        );
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let cleanup_order = order_tx.clone();
        assert!(
            queue
                .push(
                    Box::new(move || cleanup_order.send("cleanup").unwrap()),
                    true,
                    4,
                )
                .is_ok()
        );
        assert!(
            queue
                .push(
                    Box::new(move || order_tx.send("ordinary").unwrap()),
                    false,
                    4,
                )
                .is_ok()
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            order_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "ordinary"
        );
        assert_eq!(
            order_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "cleanup"
        );
        queue.stop();
        worker.join().unwrap();
    }

    #[test]
    fn queue_and_waiter_permits_bound_blocked_jobs_and_return_on_release() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = make_job(&executor, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            OperationOutcome::Succeeded(())
        });
        let mut first_handle = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&first))
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let queued_slot = executor
            .reserve_slot()
            .expect("queue capacity available while first job runs");
        let waiter_slot = executor
            .reserve_slot()
            .expect("bounded waiter capacity is available");
        assert!(
            executor.reserve_slot().is_none(),
            "all queue and waiter reservations are held"
        );
        let queued =
            make_job_with_slot(&executor, queued_slot, |_| OperationOutcome::Succeeded(()));
        let waiting =
            make_job_with_slot(&executor, waiter_slot, |_| OperationOutcome::Succeeded(()));
        let mut queued_handle = manager
            .try_submit_external(Default::default(), queued)
            .unwrap();
        let mut waiting_handle = manager
            .try_submit_external(Default::default(), waiting)
            .unwrap();

        release_tx.send(()).unwrap();
        assert!(matches!(
            first_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(matches!(
            queued_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(matches!(
            waiting_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        first_handle.wait_settlement().unwrap();
        queued_handle.wait_settlement().unwrap();
        waiting_handle.wait_settlement().unwrap();
        assert!(executor.reserve_slot().is_some());
        assert!(executor.reserve_slot().is_some());
    }

    #[test]
    fn stop_after_bind_before_activate_prevents_task_from_running() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::task::{Context, Wake, Waker};

        struct Notify(std::sync::mpsc::Sender<()>);
        impl Wake for Notify {
            fn wake(self: std::sync::Arc<Self>) {
                let _ = self.0.send(());
            }
            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                let _ = self.0.send(());
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor =
            LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let executions_for_parent = std::sync::Arc::clone(&executions);
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                let count = std::sync::Arc::clone(&executions_for_parent);
                let prepared = make_job(&executor, move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                    OperationOutcome::Succeeded(())
                });
                let id = scope.id();
                qubit_operation::PreparedExternalOperation::bind(&*prepared, id).unwrap();
                qubit_operation::OperationControl::request_stop(
                    &*prepared,
                    qubit_operation::StopReason::UserCancelled,
                );
                qubit_operation::PreparedExternalOperation::activate(
                    std::sync::Arc::clone(&prepared),
                    scope,
                )
                .unwrap();

                let (wake_tx, wake_rx) = std::sync::mpsc::channel();
                let waker = Waker::from(std::sync::Arc::new(Notify(wake_tx)));
                let mut context = Context::from_waker(&waker);
                loop {
                    match qubit_operation::PreparedExternalOperation::poll_take_outcome(
                        &*prepared,
                        &mut context,
                    ) {
                        std::task::Poll::Ready(qubit_operation::OperationOutcome::Stopped(_)) => {
                            break;
                        }
                        std::task::Poll::Ready(_) => {
                            panic!("stopped prepared task produced a non-stopped outcome")
                        }
                        std::task::Poll::Pending => {
                            wake_rx.recv_timeout(Duration::from_secs(5)).unwrap()
                        }
                    }
                }
                Ok::<_, ()>(())
            })
            .unwrap();
        let _ = parent.wait_outcome().unwrap();
        assert_eq!(executions.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn disk_permits_are_finite_and_return_when_released() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                max_disk_bytes: 8,
                ..Default::default()
            },
        )
        .unwrap();
        let permit = executor.reserve_disk(8).unwrap();
        assert!(executor.reserve_disk(1).is_none());
        drop(permit);
        assert_eq!(executor.reserve_disk(8).unwrap().amount(), 8);
    }

    #[test]
    fn duplicate_arc_submission_is_rejected_without_abandoning_claimed_job() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor =
            LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let prepared = make_job(&executor, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            OperationOutcome::Succeeded(())
        });
        let mut accepted = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&prepared))
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let rejected =
            manager.try_submit_external(Default::default(), std::sync::Arc::clone(&prepared));
        let error = match rejected {
            Ok(_) => panic!("duplicate prepared Arc was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error.reason(),
            qubit_operation::AdmissionError::AlreadyBound
        );
        release_tx.send(()).unwrap();
        assert!(matches!(
            accepted.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        accepted.wait_settlement().unwrap();
    }
}
