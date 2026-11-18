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
                let failure = local_abort_cleanup_failure(&resource, "local cleanup capacity exhausted", None);
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
                let mut native = resource.native.lock().unwrap_or_else(|error| error.into_inner());
                match native.as_mut() {
                    None => None,
                    Some(owner) => {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (resource.abort)(owner))) {
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
            *worker_state.result.lock().unwrap_or_else(|error| error.into_inner()) = Some(outcome);
            if let Some(waker) = worker_state
                .waker
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()))
            {
                std::mem::forget(payload);
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
                let failure = local_abort_cleanup_failure(&resource, "local cleanup queue is unavailable", None);
                CleanupOutcome::Failed { failure, resource }
            });
        }
        Box::pin(LocalAbortCleanupWait(state))
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::AsyncCleanup;
    use qubit_operation::CleanupContext;
    use qubit_operation::CleanupOutcome;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::super::LocalManagedAbortResource;
    use super::LocalManagedAbortCleanup;
    use crate::LocalExecutionConfig;
    use crate::LocalFileError;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalIoExecutor;

    struct ThreadWake(std::thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::park_timeout(std::time::Duration::from_millis(1)),
            }
        }
    }

    fn setup() -> (OperationManager, LocalIoExecutor, qubit_operation::ResourceId) {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_open_handles: 1,
                ..LocalExecutionConfig::default()
            },
        )
        .expect("executor");
        let id = match manager
            .run(OperationOptions::default(), |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let reference = reservation.commit(()).unwrap_or_else(|_| panic!("resource commit"));
                Ok::<_, ()>(reference.id())
            })
            .expect("identity operation")
        {
            OperationOutcome::Succeeded(id) => id,
            _ => panic!("identity operation succeeds"),
        };
        (manager, executor, id)
    }

    fn abort_ok(_: &mut u8) -> Result<(), LocalFileError> {
        Ok(())
    }

    fn abort_error(_: &mut u8) -> Result<(), LocalFileError> {
        Err(LocalFileError::new(
            LocalFileErrorKind::PermissionDenied,
            LocalFileOperation::Abort,
        ))
    }

    fn abort_panics(_: &mut u8) -> Result<(), LocalFileError> {
        panic!("abort callback panic")
    }

    struct AbortGate {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    fn abort_waits(gate: &mut AbortGate) -> Result<(), LocalFileError> {
        gate.entered.send(()).expect("test is waiting for abort start");
        gate.release.recv().expect("test releases blocked abort");
        Ok(())
    }

    fn resource(
        executor: &LocalIoExecutor,
        id: qubit_operation::ResourceId,
        abort: fn(&mut u8) -> Result<(), LocalFileError>,
    ) -> LocalManagedAbortResource<u8> {
        let resource = LocalManagedAbortResource::new(executor.reserve_cleanup().expect("cleanup permit"), abort);
        *resource.native.lock().expect("native lock") = Some(9);
        *resource.id.lock().expect("resource id lock") = Some(id);
        resource
    }

    fn cleanup_context() -> CleanupContext {
        CleanupContext::new(qubit_operation::ContextOptions::default()).expect("cleanup context")
    }

    #[test]
    fn test_capacity_exhaustion_reports_failure_and_returns_the_native_writer() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_ok);
        let reserved = resource.cleanup_permit.lock().expect("permit lock").take();
        drop(reserved);
        let full = executor.reserve_cleanup().expect("occupy cleanup capacity");

        let outcome = block_on(cleanup.cleanup(resource, cleanup_context()));
        let CleanupOutcome::Failed { failure, resource } = outcome else {
            panic!("exhausted cleanup capacity must preserve the resource");
        };
        assert_eq!(failure.resource_id, id);
        assert_eq!(failure.summary.as_str(), "local cleanup capacity exhausted");
        assert!(failure.source.is_none());
        assert_eq!(*resource.native.lock().expect("native lock"), Some(9));
        drop(full);
    }

    #[test]
    fn test_abort_error_returns_the_writer_and_preserves_the_native_error() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_error);

        let outcome = block_on(Arc::clone(&cleanup).cleanup(resource, cleanup_context()));
        let CleanupOutcome::Failed { failure, resource } = outcome else {
            panic!("failed abort must retain the writer for recovery");
        };
        assert_eq!(failure.summary.as_str(), "local writer abort cleanup failed");
        assert!(
            failure
                .source
                .expect("native abort error")
                .to_string()
                .contains("PermissionDenied")
        );
        assert_eq!(*resource.native.lock().expect("native lock"), Some(9));

        // A failed abort keeps the managed owner available for a later recovery
        // attempt. Model a transient native failure clearing, then retry it.
        let mut resource = resource;
        resource.abort = abort_ok;
        assert!(matches!(
            block_on(cleanup.cleanup(resource, cleanup_context())),
            CleanupOutcome::Released
        ));
    }

    #[test]
    fn test_abort_panic_becomes_an_indeterminate_failure_and_returns_the_writer() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_panics);

        let outcome = block_on(cleanup.cleanup(resource, cleanup_context()));
        let CleanupOutcome::Failed { failure, resource } = outcome else {
            panic!("panicking abort must retain the writer for recovery");
        };
        let source = failure.source.expect("indeterminate abort error");
        let error = source
            .downcast_ref::<LocalFileError>()
            .expect("local file error source");
        assert_eq!(error.kind(), LocalFileErrorKind::Indeterminate);
        assert_eq!(error.operation(), LocalFileOperation::Abort);
        assert_eq!(*resource.native.lock().expect("native lock"), Some(9));
    }

    #[test]
    fn test_successful_abort_releases_the_native_writer() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_ok);

        assert!(matches!(
            block_on(cleanup.cleanup(resource, cleanup_context())),
            CleanupOutcome::Released
        ));
    }

    #[test]
    fn test_abort_wait_remains_pending_until_the_worker_finishes() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<AbortGate>::new(
            executor.clone().cleanup_executor(),
        ));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let resource = LocalManagedAbortResource::new(executor.reserve_cleanup().expect("cleanup permit"), abort_waits);
        *resource.native.lock().expect("native lock") = Some(AbortGate {
            entered: entered_tx,
            release: release_rx,
        });
        *resource.id.lock().expect("resource id lock") = Some(id);

        let mut future = Box::pin(cleanup.cleanup(resource, cleanup_context()));
        entered_rx.recv().expect("worker starts the abort callback");
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));

        release_tx.send(()).expect("worker is waiting for release");
        assert!(matches!(block_on(future), CleanupOutcome::Released));
    }

    #[test]
    fn test_executor_reserves_capacity_when_the_resource_has_no_permit_or_native_writer() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_ok);
        resource.cleanup_permit.lock().expect("permit lock").take();
        resource.native.lock().expect("native lock").take();

        assert!(matches!(
            block_on(cleanup.cleanup(resource, cleanup_context())),
            CleanupOutcome::Released
        ));
    }
}
