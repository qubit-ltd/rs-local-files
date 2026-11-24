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
use crate::operation::stream::dispose_panic_payload;

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
            pending_result: Mutex::new(None),
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
            let (error, panic_payload) = {
                let mut native = resource.native.lock().unwrap_or_else(|error| error.into_inner());
                match native.as_mut() {
                    None => (None, None),
                    Some(owner) => {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (resource.abort)(owner))) {
                            Ok(Ok(())) => {
                                native.take();
                                (None, None)
                            }
                            Ok(Err(error)) => (Some(error), None),
                            Err(payload) => (
                                Some(LocalFileError::new(
                                    crate::LocalFileErrorKind::Indeterminate,
                                    crate::LocalFileOperation::Abort,
                                )),
                                Some(payload),
                            ),
                        }
                    }
                }
            };
            if let Some(payload) = panic_payload {
                dispose_panic_payload(payload);
            }
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
                .pending_result
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(outcome);
        });
        let completion_state = Arc::clone(&state);
        let completion = Box::new(move || publish_abort_result(&completion_state));
        if self
            .executor
            .enqueue_cleanup_with_completion(job, permit, completion)
            .is_err()
        {
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

/// Publishes an abort result after the cleanup worker returns its capacity.
///
/// # Parameters
///
/// * `state` — result and waiter state shared with the cleanup future.
///
/// # Type Parameters
/// * `W` — native writer type retained by the cleanup state.
fn publish_abort_result<W: Send + 'static>(state: &LocalAbortCleanupState<W>) {
    if let Some(result) = state
        .pending_result
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        *state.result.lock().unwrap_or_else(|error| error.into_inner()) = Some(result);
    }
    let waker = state.waker.lock().unwrap_or_else(|error| error.into_inner()).take();
    if let Some(waker) = waker
        && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()))
    {
        dispose_panic_payload(payload);
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
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

    use super::super::LocalAbortCleanupState;
    use super::super::LocalManagedAbortResource;
    use super::LocalManagedAbortCleanup;
    use super::publish_abort_result;
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

    struct PanicWake(mpsc::Sender<()>);

    impl Wake for PanicWake {
        fn wake(self: Arc<Self>) {
            let _ = self.0.send(());
            std::panic::panic_any(());
        }

        fn wake_by_ref(self: &Arc<Self>) {
            let _ = self.0.send(());
            std::panic::panic_any(());
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

    static PANIC_PAYLOAD_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct SecondaryAbortPanicPayload;
    impl Drop for SecondaryAbortPanicPayload {
        fn drop(&mut self) {
            PANIC_PAYLOAD_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct AbortPanicPayload;
    impl Drop for AbortPanicPayload {
        fn drop(&mut self) {
            PANIC_PAYLOAD_DROPS.fetch_add(1, Ordering::SeqCst);
            std::panic::panic_any(SecondaryAbortPanicPayload);
        }
    }

    fn abort_panics_with_panicking_payload(_: &mut u8) -> Result<(), LocalFileError> {
        std::panic::panic_any(AbortPanicPayload);
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
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&resource.id);
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&resource.cleanup_permit);
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
    fn test_stopped_cleanup_worker_returns_the_native_writer() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_ok);
        executor.inner.queue.stop();

        let CleanupOutcome::Failed { failure, resource } = block_on(cleanup.cleanup(resource, cleanup_context()))
        else {
            panic!("rejected abort must return the native writer");
        };
        assert_eq!(failure.resource_id, id);
        assert_eq!(failure.summary.as_str(), "local cleanup queue is unavailable");
        assert_eq!(*resource.native.lock().expect("native lock"), Some(9));
    }

    #[test]
    fn test_cleanup_without_native_writer_releases_reserved_capacity() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_panics);
        assert_eq!(resource.native.lock().expect("native lock").take(), Some(9));
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&resource.native);
        assert!(matches!(
            block_on(cleanup.cleanup(resource, cleanup_context())),
            CleanupOutcome::Released
        ));
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
        // attempt. The published failure guarantees the worker already returned
        // its permit, so recovery can retry through the cleanup policy directly.
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

    #[cfg(panic = "unwind")]
    #[test]
    fn test_abort_payload_destructor_panic_still_publishes_recoverable_writer() {
        PANIC_PAYLOAD_DROPS.store(0, Ordering::SeqCst);
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_panics_with_panicking_payload);

        let outcome = block_on(Arc::clone(&cleanup).cleanup(resource, cleanup_context()));
        let CleanupOutcome::Failed { failure, mut resource } = outcome else {
            panic!("abort panic must publish failure with its original writer");
        };
        let source = failure.source.expect("indeterminate abort error");
        let error = source
            .downcast_ref::<LocalFileError>()
            .expect("local file error source");
        assert_eq!(LocalFileErrorKind::Indeterminate, error.kind());
        assert_eq!(LocalFileOperation::Abort, error.operation());
        assert_eq!(2, PANIC_PAYLOAD_DROPS.load(Ordering::SeqCst));
        assert_eq!(Some(9), *resource.native.lock().expect("original native writer"));

        resource.abort = abort_ok;
        assert!(matches!(
            block_on(cleanup.cleanup(resource, cleanup_context())),
            CleanupOutcome::Released
        ));
    }

    #[test]
    fn test_successful_abort_releases_the_native_writer() {
        let (_manager, executor, id) = setup();
        let cleanup = Arc::new(LocalManagedAbortCleanup::<u8>::new(executor.cleanup_executor()));
        let resource = resource(&executor, id, abort_ok);

        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&resource.native);
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
        assert!(
            executor.reserve_cleanup().is_some(),
            "published abort result must follow cleanup permit return"
        );
    }

    #[test]
    fn test_publish_abort_result_recovers_poisoned_publication_locks() {
        let state = Arc::new(LocalAbortCleanupState {
            owner: std::sync::Mutex::new(None::<LocalManagedAbortResource<u8>>),
            result: std::sync::Mutex::new(None),
            pending_result: std::sync::Mutex::new(Some(CleanupOutcome::Released)),
            waker: std::sync::Mutex::new(Some(Waker::noop().clone())),
        });
        for lock in [0, 1, 2] {
            let state = Arc::clone(&state);
            let poisoner = std::thread::spawn(move || {
                match lock {
                    0 => drop(state.pending_result.lock().expect("pending result lock")),
                    1 => drop(state.result.lock().expect("published result lock")),
                    _ => drop(state.waker.lock().expect("cleanup waker lock")),
                }
                panic!("poison abort publication lock");
            });
            assert!(poisoner.join().is_err());
        }

        publish_abort_result(&state);

        assert!(matches!(
            state.result.lock().unwrap_or_else(|error| error.into_inner()).take(),
            Some(CleanupOutcome::Released)
        ));
        assert!(
            state
                .pending_result
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
        );
        assert!(state.waker.lock().unwrap_or_else(|error| error.into_inner()).is_none());
    }

    #[test]
    fn test_panicking_waker_does_not_lose_cleanup_completion() {
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
        let (woken_tx, woken_rx) = mpsc::channel();
        let panic_waker = Waker::from(Arc::new(PanicWake(woken_tx)));
        let mut panic_context = Context::from_waker(&panic_waker);
        assert!(matches!(future.as_mut().poll(&mut panic_context), Poll::Pending));

        release_tx.send(()).expect("worker is waiting for release");
        woken_rx.recv().expect("worker invokes the registered waker");
        let normal_waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut normal_context = Context::from_waker(&normal_waker);
        assert!(matches!(
            future.as_mut().poll(&mut normal_context),
            Poll::Ready(CleanupOutcome::Released)
        ));
    }

    #[test]
    fn test_abort_completion_recovers_poisoned_publication_locks() {
        let state = LocalAbortCleanupState::<AbortGate> {
            owner: std::sync::Mutex::new(None),
            result: std::sync::Mutex::new(None),
            pending_result: std::sync::Mutex::new(None),
            waker: std::sync::Mutex::new(None),
        };
        #[cfg(panic = "unwind")]
        {
            crate::operation::poison_test_mutex(&state.pending_result);
            crate::operation::poison_test_mutex(&state.result);
            crate::operation::poison_test_mutex(&state.waker);
        }
        *state
            .pending_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CleanupOutcome::Released);
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        *state.waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(waker);

        publish_abort_result(&state);

        assert!(matches!(
            state
                .result
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref(),
            Some(CleanupOutcome::Released)
        ));
        assert!(
            state
                .waker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        );
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
