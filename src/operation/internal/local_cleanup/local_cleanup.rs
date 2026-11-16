// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_operation::AsyncCleanup;
use qubit_operation::CleanupContext;
use qubit_operation::CleanupFuture;
use qubit_operation::CleanupOutcome;

use super::CleanupState;
use super::cleanup_failure;
use super::cleanup_resource;
use super::safe_wake;
use crate::operation::executor::LocalIoExecutor;
use crate::operation::temp::LocalTempResource;

/// Runs native temporary-resource cleanup on the executor's reserved cleanup
/// queue.
///
/// The reservation keeps cleanup capacity available even when ordinary work has
/// filled the executor queue, so dropping an operation does not need to perform
/// filesystem I/O on the caller's thread.
pub struct LocalCleanup {
    /// Executor that owns the dedicated cleanup worker and queue.
    pub(super) executor: LocalIoExecutor,
}

impl AsyncCleanup<LocalTempResource> for LocalCleanup {
    /// Transfers the resource to the cleanup worker and waits for its result.
    ///
    /// If no cleanup permit is available or enqueueing fails, the original
    /// resource is returned in a failed outcome so the caller can recover it.
    ///
    /// # Parameters
    ///
    /// * `self` — shared cleanup policy used to access the executor.
    /// * `resource` — registered owner whose native handle must be released.
    /// * `_context` — framework cleanup context; cleanup does not depend on
    ///   cancellation state.
    fn cleanup(
        self: Arc<Self>,
        resource: LocalTempResource,
        _context: CleanupContext,
    ) -> CleanupFuture<LocalTempResource> {
        let reserved = resource.cleanup_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(permit) = reserved.or_else(|| self.executor.reserve_cleanup()) else {
            let failure = cleanup_failure(&resource, "local cleanup queue capacity exhausted", None);
            return Box::pin(async move { CleanupOutcome::Failed { failure, resource } });
        };
        let state = Arc::new(CleanupState {
            result: Mutex::new(None),
            waker: Mutex::new(None),
            owner: Mutex::new(Some(resource)),
        });
        let worker_state = Arc::clone(&state);
        let job = Box::new(move || {
            let Some(resource) = worker_state.owner.lock().unwrap_or_else(|e| e.into_inner()).take() else {
                return;
            };
            let result = cleanup_resource(resource);
            *worker_state.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            let waker = worker_state.waker.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(waker) = waker {
                safe_wake(waker);
            }
        });
        if let Err((_job, _permit)) = self.executor.enqueue_cleanup(job, permit) {
            let resource = state
                .owner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("cleanup owner remains reserved when enqueue is rejected");
            let failure = cleanup_failure(&resource, "local cleanup worker is unavailable", None);
            return Box::pin(async move { CleanupOutcome::Failed { failure, resource } });
        }
        Box::pin(CleanupWait {
            state,
            marker: std::marker::PhantomData,
        })
    }
}
