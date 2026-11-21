// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::super::internal::permits::Permit;
use super::super::internal::permits::PermitPool;
use super::super::internal::worker::CleanupCompletionEnqueueError;
use super::super::internal::worker::CleanupEnqueueError;
use super::super::internal::worker::WorkerQueue;
use super::worker_owner::WorkerOwner;

/// Keeps cleanup workers alive without retaining their operation group.
#[derive(Clone)]
pub(crate) struct LocalCleanupExecutor {
    /// Worker queue used for reserved cleanup jobs.
    pub(super) queue: Arc<WorkerQueue>,
    /// Capacity pool used to bound cleanup jobs.
    pub(super) cleanup_slots: Arc<PermitPool>,
    /// Worker lifetime owner retained while recovery may retry cleanup.
    pub(super) _owner: Arc<WorkerOwner>,
    /// Maximum number of cleanup jobs admitted to the queue.
    pub(super) cleanup_capacity: usize,
}

impl LocalCleanupExecutor {
    /// Reserves one slot for a cleanup job.
    ///
    /// # Returns
    /// A permit when cleanup capacity is available.
    pub(crate) fn reserve_cleanup(&self) -> Option<Permit> {
        self.cleanup_slots.try_acquire(1)
    }

    /// Enqueues a cleanup job using its reserved capacity.
    ///
    /// # Parameters
    /// - `job` — Cleanup operation to run on a local worker.
    /// - `permit` — Capacity reserved for this cleanup operation.
    ///
    /// # Returns
    /// `Ok(())` when queued, or the enqueue error when the worker queue stopped.
    #[allow(dead_code)]
    pub(crate) fn enqueue_cleanup(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
    ) -> Result<(), CleanupEnqueueError> {
        self.queue.push_cleanup(job, permit, self.cleanup_capacity)
    }

    /// Enqueues cleanup work whose completion runs after its permit is returned.
    ///
    /// # Parameters
    /// - `job` — Cleanup operation to run on a local worker.
    /// - `permit` — Capacity reserved for this cleanup operation.
    /// - `completion` — Callback used to publish the completed cleanup result.
    ///
    /// # Returns
    /// `Ok(())` when queued, or the job, permit, and callback when rejected.
    pub(crate) fn enqueue_cleanup_with_completion(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
        completion: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), CleanupCompletionEnqueueError> {
        self.queue
            .push_cleanup_with_completion(job, permit, completion, self.cleanup_capacity)
    }
}
