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
        let mut workers = Vec::with_capacity(config.worker_count);
        for index in 0..config.worker_count {
            let worker_queue = Arc::clone(&queue);
            match thread::Builder::new()
                .name(format!("qubit-local-io-{index}"))
                .spawn(move || worker_queue.run())
            {
                Ok(handle) => workers.push(handle),
                Err(error) => {
                    queue.stop();
                    for handle in workers {
                        let _ = handle.join();
                    }
                    return Err(LocalExecutorCreateError::WorkerStart(error));
                }
            }
        }
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
    use std::sync::mpsc;
    use std::time::Duration;

    use super::LocalExecutionConfig;
    use super::LocalIoExecutor;
    use crate::operation::internal::prepared_local_job::PreparedLocalJob;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

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
        let first_slot = executor.reserve_slot().unwrap();
        let first = PreparedLocalJob::<(), (), ()>::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            first_slot,
            executor.reserve_bytes(1).unwrap(),
            executor.reserve_handle().unwrap(),
            move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                OperationOutcome::Succeeded(())
            },
        );
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
        let queued = PreparedLocalJob::<(), (), ()>::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            queued_slot,
            executor.reserve_bytes(1).unwrap(),
            executor.reserve_handle().unwrap(),
            |_| OperationOutcome::Succeeded(()),
        );
        let waiting = PreparedLocalJob::<(), (), ()>::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            waiter_slot,
            executor.reserve_bytes(1).unwrap(),
            executor.reserve_handle().unwrap(),
            |_| OperationOutcome::Succeeded(()),
        );
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
                let prepared = PreparedLocalJob::<(), (), ()>::new(
                    std::sync::Arc::clone(&executor.inner),
                    (),
                    executor.reserve_slot().unwrap(),
                    executor.reserve_bytes(1).unwrap(),
                    executor.reserve_handle().unwrap(),
                    move |_| {
                        count.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Succeeded(())
                    },
                );
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
    fn duplicate_arc_submission_is_rejected_without_abandoning_claimed_job() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor =
            LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let prepared = PreparedLocalJob::<(), (), ()>::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            executor.reserve_slot().unwrap(),
            executor.reserve_bytes(1).unwrap(),
            executor.reserve_handle().unwrap(),
            move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                OperationOutcome::Succeeded(())
            },
        );
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
