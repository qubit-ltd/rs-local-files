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

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Waker;

    use qubit_operation::AsyncCleanup;

    use super::super::worker_closable::WorkerClosable;
    use super::WorkerCleanup;

    struct TestResource {
        requests: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        result: Poll<io::Result<()>>,
        resource_id: Arc<std::sync::Mutex<Option<qubit_operation::ResourceId>>>,
    }

    impl Drop for TestResource {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl WorkerClosable for TestResource {
        fn request_cleanup(&self) {
            self.requests.fetch_add(1, Ordering::SeqCst);
        }

        fn poll_cleanup(&self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match &self.result {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
                Poll::Ready(Err(error)) => Poll::Ready(Err(io::Error::new(error.kind(), error.to_string()))),
            }
        }

        fn resource_id(&self) -> Arc<std::sync::Mutex<Option<qubit_operation::ResourceId>>> {
            Arc::clone(&self.resource_id)
        }
    }

    fn resource(
        result: Poll<io::Result<()>>,
        id: Option<qubit_operation::ResourceId>,
    ) -> (TestResource, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let requests = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let resource_id = Arc::new(std::sync::Mutex::new(id));
        (
            TestResource {
                requests: Arc::clone(&requests),
                drops: Arc::clone(&drops),
                result,
                resource_id,
            },
            requests,
            drops,
        )
    }

    fn context() -> qubit_operation::CleanupContext {
        qubit_operation::CleanupContext::new(qubit_operation::ContextOptions::default()).unwrap()
    }

    #[test]
    fn cleanup_retains_resource_until_worker_confirmation() {
        let (pending_resource, requests, drops) = resource(Poll::Pending, None);
        let mut cleanup = Box::pin(WorkerCleanup::<TestResource>::policy().cleanup(pending_resource, context()));
        let mut cx = Context::from_waker(Waker::noop());

        assert!(cleanup.as_mut().poll(&mut cx).is_pending());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(cleanup);
        assert_eq!(drops.load(Ordering::SeqCst), 1);

        let (resource, requests, drops) = resource(Poll::Ready(Ok(())), None);
        let mut cleanup = Box::pin(WorkerCleanup::<TestResource>::policy().cleanup(resource, context()));
        assert!(matches!(
            cleanup.as_mut().poll(&mut cx),
            Poll::Ready(qubit_operation::CleanupOutcome::Released)
        ));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dispose_requests_cleanup_and_drops_resource() {
        let (resource, requests, drops) = resource(Poll::Pending, None);
        WorkerCleanup::<TestResource>::dispose(resource);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cleanup_failure_returns_resource_with_failure_identity() {
        let manager = qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default())
            .expect("operation manager should be created");
        let resource_id = Arc::new(std::sync::Mutex::new(None));
        let id_for_operation = Arc::clone(&resource_id);
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let resource_scope = scope
                    .resource_scope(qubit_operation::ResourceScopeOptions { capacity: 1 })
                    .expect("resource scope should be created");
                let managed = resource_scope.manage(()).expect("resource should be registered");
                *id_for_operation.lock().unwrap() = Some(managed.id());
                drop(managed);
                resource_scope.end();
                Ok::<_, ()>(())
            })
            .expect("operation should settle");
        drop(completion);
        let expected_id = resource_id.lock().unwrap().expect("registered resource ID");

        let (resource, requests, drops) = resource(
            Poll::Ready(Err(io::Error::other("worker cleanup failed"))),
            Some(expected_id),
        );
        let mut cleanup = Box::pin(WorkerCleanup::<TestResource>::policy().cleanup(resource, context()));
        let mut cx = Context::from_waker(Waker::noop());
        let outcome = cleanup.as_mut().poll(&mut cx);

        match outcome {
            Poll::Ready(qubit_operation::CleanupOutcome::Failed { failure, resource }) => {
                assert_eq!(failure.resource_id, expected_id);
                assert_eq!(failure.stage, qubit_operation::CleanupStage::AsyncRelease);
                assert_eq!(requests.load(Ordering::SeqCst), 1);
                assert_eq!(drops.load(Ordering::SeqCst), 0);
                drop(resource);
                assert_eq!(drops.load(Ordering::SeqCst), 1);
            }
            _ => panic!("expected failed cleanup outcome"),
        }
    }
}
