// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::task::Poll;

use super::WorkerClosable;

/// Asynchronous release policy for a worker-owned native resource.
///
/// # Type Parameters
///
/// * `T` - Managed resource whose worker must confirm cleanup before release.
pub(super) struct WorkerCleanup<T>(
    /// Associates the cleanup policy with its managed resource type.
    std::marker::PhantomData<fn(T)>,
);

impl<T: WorkerClosable> WorkerCleanup<T> {
    /// Creates the shared cleanup policy used by the operation resource scope.
    pub(super) fn policy() -> Arc<Self> {
        Arc::new(Self(std::marker::PhantomData))
    }

    /// Releases a resource immediately after registration fails.
    ///
    /// # Parameters
    ///
    /// * `resource` - Resource whose terminal cleanup must be queued.
    pub(super) fn dispose(resource: T) {
        resource.request_cleanup();
        // The queued terminal command retains the driver and native owner.
        drop(resource);
    }
}

impl<T: WorkerClosable> qubit_operation::AsyncCleanup<T> for WorkerCleanup<T> {
    /// Polls worker cleanup while preserving ownership until confirmation.
    ///
    /// # Parameters
    ///
    /// * `self` - Shared cleanup policy associated with the resource type.
    /// * `resource` - Managed resource retained until close is confirmed.
    /// * `_context` - Operation cleanup context, unused by worker cleanup.
    fn cleanup(
        self: Arc<Self>,
        resource: T,
        _context: qubit_operation::CleanupContext,
    ) -> qubit_operation::CleanupFuture<T> {
        let resource_id = resource.resource_id();
        resource.request_cleanup();
        let mut resource = Some(resource);
        Box::pin(std::future::poll_fn(move |cx| {
            match resource.as_ref().expect("cleanup resource retained").poll_cleanup(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(())) => {
                    drop(resource.take());
                    Poll::Ready(qubit_operation::CleanupOutcome::Released)
                }
                Poll::Ready(Err(error)) => {
                    let id = resource_id
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .expect("managed worker resource has a ResourceId");
                    let failure = qubit_operation::CleanupFailure {
                        resource_id: id,
                        stage: qubit_operation::CleanupStage::AsyncRelease,
                        summary: qubit_operation::DiagnosticSummary::new(&error.to_string()),
                        source: Some(Arc::new(error)),
                    };
                    Poll::Ready(qubit_operation::CleanupOutcome::Failed {
                        failure,
                        resource: resource.take().expect("cleanup resource retained"),
                    })
                }
            }
        }))
    }
}
