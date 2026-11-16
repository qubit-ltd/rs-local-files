// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use qubit_operation::AsyncCleanup;
use qubit_operation::CleanupContext;
use qubit_operation::CleanupFuture;
use qubit_operation::CleanupOutcome;

use super::LocalAbortCleanupState;
use super::LocalAbortCleanupWait;
use super::LocalManagedAbortCleanup;
use super::LocalManagedAbortResource;
use super::local_abort_cleanup_failure;
use crate::LocalFileError;

impl<W: Send + 'static> AsyncCleanup<LocalManagedAbortResource<W>> for LocalManagedAbortCleanup<W> {
    /// Runs the native abort callback on a reserved cleanup worker.
    ///
    /// If no cleanup permit is available, the resource is returned with a
    /// capacity failure so a later cleanup attempt can retry. A worker panic
    /// becomes an indeterminate abort failure; successful abort releases the
    /// managed native writer, while callback errors preserve it for recovery.
    ///
    /// # Parameters
    ///
    /// * `self` - Shared cleanup policy and worker executor.
    /// * `resource` - Native writer owner whose pending abort is serviced.
    /// * `_context` - Cleanup context; this implementation relies on the
    ///   resource's reserved permit and does not inspect additional context.
    fn cleanup(
        self: Arc<Self>,
        resource: LocalManagedAbortResource<W>,
        _context: CleanupContext,
    ) -> CleanupFuture<LocalManagedAbortResource<W>> {
        let permit = resource
            .cleanup_permit
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .or_else(|| self.executor.reserve_cleanup());
        let Some(permit) = permit else {
            return Box::pin(async move {
                let failure = local_abort_cleanup_failure(
                    &resource,
                    "local cleanup capacity exhausted",
                    None,
                );
                CleanupOutcome::Failed { failure, resource }
            });
        };
        let state = Arc::new(LocalAbortCleanupState {
            owner: Mutex::new(Some(resource)),
            result: Mutex::new(None),
            waker: Mutex::new(None),
        });
        let worker_state = Arc::clone(&state);
        let job = Box::new(move || {
            let Some(resource) = worker_state
                .owner
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            else {
                return;
            };
            let error = {
                let mut native = resource
                    .native
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                match native.as_mut() {
                    None => None,
                    Some(owner) => {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            (resource.abort)(owner)
                        })) {
                            Ok(Ok(())) => {
                                native.take();
                                None
                            }
                            Ok(Err(error)) => Some(error),
                            Err(_) => Some(LocalFileError::new(
                                crate::LocalFileErrorKind::Indeterminate,
                                crate::LocalFileOperation::Abort,
                            )),
                        }
                    }
                }
            };
            let outcome = match error {
                None => CleanupOutcome::Released,
                Some(error) => {
                    let failure = local_abort_cleanup_failure(
                        &resource,
                        "local writer abort cleanup failed",
                        Some(Arc::new(error)),
                    );
                    CleanupOutcome::Failed { failure, resource }
                }
            };
            *worker_state
                .result
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(outcome);
            if let Some(waker) = worker_state
                .waker
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    waker.wake();
                })) {
                    std::mem::forget(payload);
                }
            }
        });
        if self.executor.enqueue_cleanup(job, permit).is_err() {
            let resource = state
                .owner
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .expect("cleanup owner remains available when queue admission fails");
            return Box::pin(async move {
                let failure = local_abort_cleanup_failure(
                    &resource,
                    "local cleanup queue is unavailable",
                    None,
                );
                CleanupOutcome::Failed { failure, resource }
            });
        }
        Box::pin(LocalAbortCleanupWait(state))
    }
}
