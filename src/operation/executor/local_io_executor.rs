// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use qubit_operation::GroupOptions;
use qubit_operation::OperationManager;
use qubit_operation::ShutdownHandle;
use qubit_operation::ShutdownMode;

use super::super::LocalExecutionConfig;
use super::super::LocalExecutorConfigError;
use super::super::LocalExecutorCreateError;
use super::super::internal::permits::DiskPermit;
use super::super::internal::permits::DiskPermitPool;
use super::super::internal::permits::Permit;
use super::super::internal::permits::PermitPool;
use super::super::internal::worker::CleanupCompletionEnqueueError;
use super::super::internal::worker::CleanupEnqueueError;
use super::super::internal::worker::WorkerQueue;
use super::executor_inner::ExecutorInner;
use super::local_cleanup_executor::LocalCleanupExecutor;
use super::worker_owner::WorkerOwner;
/// Bounded local I/O worker group associated with an application manager.
#[derive(Clone)]
pub struct LocalIoExecutor {
    /// Stores the inner associated with this value.
    pub(crate) inner: Arc<ExecutorInner>,
    /// Stores the  owner associated with this value.
    pub(super) _owner: Arc<WorkerOwner>,
}

impl LocalIoExecutor {
    /// Starts a bounded worker group attached to `manager`.
    ///
    /// All worker and capacity fields in `config` must be nonzero. Ordinary
    /// submissions acquire either a queue or waiter permit and count against
    /// the operation group's combined queue-plus-waiter capacity. Dropping the
    /// last executor owner stops admission and drains accepted work before the
    /// workers exit.
    /// # Parameters
    /// - `manager` — operation manager that owns the local worker group.
    /// - `config` — finite, nonzero worker and resource limits shared by all
    ///   executor clones.
    pub fn new(manager: OperationManager, config: LocalExecutionConfig) -> Result<Self, LocalExecutorCreateError> {
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
        let owner = Arc::new(WorkerOwner::new(Arc::clone(&queue), workers));
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
                cleanup_slots: Arc::new(PermitPool::new(config.max_open_handles)),
            }),
            _owner: owner,
        })
    }
    /// Requests shutdown of this executor's operation group.
    ///
    /// The request closes this group's admission and drains or cancels its
    /// accepted operations according to `mode`. It does not shut down the
    /// application manager; callers may continue submitting through other
    /// manager views. Drop the executor after observing shutdown completion to
    /// stop and join its local workers.
    ///
    /// # Parameters
    ///
    /// - `mode` — whether accepted group operations drain or receive
    ///   cancellation requests.
    ///
    /// # Returns
    ///
    /// A handle for observing completion of the group's shutdown.
    pub fn request_shutdown(&self, mode: ShutdownMode) -> ShutdownHandle {
        self.inner.group.request_shutdown(mode)
    }

    /// Returns a cleanup-only worker handle suitable for retained recovery
    /// policies. It keeps workers alive without retaining the operation group
    /// or its manager.
    pub(crate) fn cleanup_executor(&self) -> LocalCleanupExecutor {
        LocalCleanupExecutor {
            queue: Arc::clone(&self.inner.queue),
            cleanup_slots: Arc::clone(&self.inner.cleanup_slots),
            _owner: Arc::clone(&self._owner),
            cleanup_capacity: self.inner.config.max_open_handles,
        }
    }

    /// Returns the immutable resource and queue limits used by this executor.
    pub fn config(&self) -> &LocalExecutionConfig {
        &self.inner.config
    }
    /// Runs the reserve slot behavior while preserving the operation lifecycle
    /// invariants.
    pub(crate) fn reserve_slot(&self) -> Option<Permit> {
        self.inner
            .queue_slots
            .try_acquire(1)
            .or_else(|| self.inner.waiter_slots.try_acquire(1))
    }
    /// Runs the reserve bytes behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `amount` — supplies data or policy for this operation.
    pub(crate) fn reserve_bytes(&self, amount: usize) -> Option<Permit> {
        self.inner.byte_slots.try_acquire(amount)
    }
    /// Runs the reserve handle behavior while preserving the operation
    /// lifecycle invariants.
    pub(crate) fn reserve_handle(&self) -> Option<Permit> {
        self.inner.handle_slots.try_acquire(1)
    }
    /// Atomically reserves ordinary handles for a multi-reader operation.
    ///
    /// # Parameters
    /// - `amount` — number of handles to reserve as one indivisible request.
    pub(crate) fn reserve_handles(&self, amount: usize) -> Option<Permit> {
        self.inner.handle_slots.try_acquire(amount)
    }
    /// Runs the reserve disk behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `amount` — supplies data or policy for this operation.
    #[allow(dead_code)]
    pub(crate) fn reserve_disk(&self, amount: u64) -> Option<DiskPermit> {
        self.inner.disk_slots.try_acquire(amount)
    }
    /// Runs the reserve fact behavior while preserving the operation lifecycle
    /// invariants.
    pub(crate) fn reserve_fact(&self) -> Option<Permit> {
        self.inner.fact_slots.try_acquire(1)
    }
    /// Runs the enqueue task behavior while preserving the operation lifecycle
    /// invariants.
    /// # Type Parameters
    /// - `F` — type used by this operation.
    /// # Parameters
    /// - `task` — supplies data or policy for this operation.
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
    /// Runs the reserve cleanup behavior while preserving the operation
    /// lifecycle invariants.
    #[allow(dead_code)]
    pub(crate) fn reserve_cleanup(&self) -> Option<Permit> {
        self.inner.cleanup_slots.try_acquire(1)
    }
    /// Runs the enqueue cleanup behavior while preserving the operation
    /// lifecycle invariants.
    /// # Parameters
    /// - `job` — supplies data or policy for this operation.
    /// - `permit` — supplies data or policy for this operation.
    #[allow(dead_code)]
    pub(crate) fn enqueue_cleanup(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
    ) -> Result<(), CleanupEnqueueError> {
        self.inner
            .queue
            .push_cleanup(job, permit, self.inner.config.max_open_handles)
    }

    /// Enqueues cleanup whose completion callback runs after its permit is
    /// returned to the cleanup pool.
    ///
    /// # Parameters
    /// - `job` — cleanup action to run on the worker.
    /// - `permit` — reserved cleanup queue capacity.
    /// - `completion` — notification run after the permit is released.
    pub(crate) fn enqueue_cleanup_with_completion(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
        completion: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), CleanupCompletionEnqueueError> {
        self.inner
            .queue
            .push_cleanup_with_completion(job, permit, completion, self.inner.config.max_open_handles)
    }
}
/// Runs the spawn workers behavior while preserving the operation lifecycle
/// invariants.
/// # Type Parameters
/// - `F` — type used by this operation.
/// # Parameters
/// - `queue` — supplies data or policy for this operation.
/// - `count` — supplies data or policy for this operation.
/// - `spawn` — supplies data or policy for this operation.
pub(super) fn spawn_workers<F>(queue: &Arc<WorkerQueue>, count: usize, mut spawn: F) -> io::Result<Vec<JoinHandle<()>>>
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
/// Runs the validate behavior while preserving the operation lifecycle
/// invariants.
/// # Parameters
/// - `config` — supplies data or policy for this operation.
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
