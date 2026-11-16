use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use qubit_operation::{AsyncCleanup, CleanupContext, CleanupFuture, CleanupOutcome};

use super::LocalTempNative;
use super::LocalTempResource;
use super::safe_clone_waker;
use super::safe_drop_waker;
use crate::operation::executor::LocalIoExecutor;

/// Asynchronous cleanup policy using the executor's dedicated cleanup queue.
pub struct LocalCleanup {
    pub(super) executor: LocalIoExecutor,
}

impl AsyncCleanup<LocalTempResource> for LocalCleanup {
    fn cleanup(
        self: Arc<Self>,
        resource: LocalTempResource,
        _context: CleanupContext,
    ) -> CleanupFuture<LocalTempResource> {
        let reserved = resource
            .cleanup_permit
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let Some(permit) = reserved.or_else(|| self.executor.reserve_cleanup()) else {
            let failure =
                cleanup_failure(&resource, "local cleanup queue capacity exhausted", None);
            return Box::pin(async move { CleanupOutcome::Failed { failure, resource } });
        };
        let state = Arc::new(CleanupState {
            result: Mutex::new(None),
            waker: Mutex::new(None),
            owner: Mutex::new(Some(resource)),
        });
        let worker_state = Arc::clone(&state);
        let job = Box::new(move || {
            let Some(resource) = worker_state
                .owner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            else {
                return;
            };
            let result = cleanup_resource(resource);
            *worker_state
                .result
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(result);
            let waker = worker_state
                .waker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
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

struct CleanupState {
    result: Mutex<Option<Option<(crate::LocalFileError, LocalTempResource)>>>,
    waker: Mutex<Option<Waker>>,
    owner: Mutex<Option<LocalTempResource>>,
}
struct CleanupWait {
    state: Arc<CleanupState>,
    marker: std::marker::PhantomData<LocalTempResource>,
}

impl Future for CleanupWait {
    type Output = CleanupOutcome<LocalTempResource>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = self
            .state
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(result) = result {
            clear_waker(&self.state);
            return Poll::Ready(cleanup_outcome(result));
        }
        let Some(new_waker) = safe_clone_waker(cx.waker()) else {
            let result = self
                .state
                .result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            return match result {
                Some(result) => {
                    clear_waker(&self.state);
                    Poll::Ready(cleanup_outcome(result))
                }
                None => Poll::Pending,
            };
        };
        let old_waker = {
            let mut waker = self.state.waker.lock().unwrap_or_else(|e| e.into_inner());
            waker.replace(new_waker)
        };
        safe_drop_waker(old_waker);
        let result = self
            .state
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(result) = result {
            clear_waker(&self.state);
            Poll::Ready(cleanup_outcome(result))
        } else {
            Poll::Pending
        }
    }
}

fn clear_waker(state: &CleanupState) {
    let waker = state.waker.lock().unwrap_or_else(|e| e.into_inner()).take();
    safe_drop_waker(waker);
}

fn safe_wake(waker: Waker) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake())) {
        // A panic payload can itself panic on Drop. Preserve the cleanup owner
        // even for adversarial waker implementations.
        std::mem::forget(payload);
    }
}

fn cleanup_outcome(
    result: Option<(crate::LocalFileError, LocalTempResource)>,
) -> CleanupOutcome<LocalTempResource> {
    match result {
        None => CleanupOutcome::Released,
        Some((error, resource)) => {
            let failure = cleanup_failure(
                &resource,
                "native temporary cleanup failed",
                Some(Arc::new(error)),
            );
            CleanupOutcome::Failed { failure, resource }
        }
    }
}

fn cleanup_resource(
    resource: LocalTempResource,
) -> Option<(crate::LocalFileError, LocalTempResource)> {
    let result = {
        let mut native = resource.native.lock().unwrap_or_else(|e| e.into_inner());
        match native.as_mut() {
            Some(LocalTempNative::File(value)) => value.cleanup().map(|()| {
                native.take();
                ()
            }),
            Some(LocalTempNative::Sealed(value)) => value.cleanup().map(|()| {
                native.take();
                ()
            }),
            Some(LocalTempNative::Directory(value)) => value.cleanup().map(|()| {
                native.take();
                ()
            }),
            Some(LocalTempNative::Recovery(value)) => value.cleanup().map(|()| {
                native.take();
                ()
            }),
            Some(LocalTempNative::Unconfirmed) => Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::Cleanup,
                None,
                None,
                std::io::Error::other(
                    "native temp owner was lost during a panicked transition; release is unconfirmed",
                ),
            )),
            None => Ok(()),
        }
    };
    result.err().map(|error| (error, resource))
}

fn cleanup_failure(
    resource: &LocalTempResource,
    summary: &str,
    source: Option<Arc<dyn std::error::Error + Send + Sync>>,
) -> qubit_operation::CleanupFailure {
    qubit_operation::CleanupFailure {
        resource_id: resource
            .id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .expect("managed temp resource has an id before cleanup"),
        stage: qubit_operation::CleanupStage::Drop,
        summary: qubit_operation::DiagnosticSummary::new(summary),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalTempResource;
    use crate::operation::temp::LocalTempNative;
    use crate::options::LocalTempFileOptions;
    use qubit_operation::{
        CleanupContext, ManagerOptions, OperationManager, OperationOptions, OperationOutcome,
    };
    use std::task::{RawWaker, RawWakerVTable};

    struct DropPanics;
    struct ThreadWake(std::thread::Thread);
    impl std::task::Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    impl std::task::Wake for DropPanics {
        fn wake(self: Arc<Self>) {}
    }
    impl Drop for DropPanics {
        fn drop(&mut self) {
            panic!("custom waker drop panic");
        }
    }

    unsafe fn clone_panics(_: *const ()) -> RawWaker {
        panic!("custom waker clone panic");
    }
    unsafe fn raw_noop(_: *const ()) {}
    unsafe fn wake_panics(_: *const ()) {
        panic!("custom waker wake panic");
    }
    static CLONE_PANIC_VTABLE: RawWakerVTable =
        RawWakerVTable::new(clone_panics, raw_noop, raw_noop, raw_noop);
    static WAKE_PANIC_VTABLE: RawWakerVTable =
        RawWakerVTable::new(raw_clone, wake_panics, wake_panics, raw_noop);

    unsafe fn raw_clone(data: *const ()) -> RawWaker {
        RawWaker::new(data, &WAKE_PANIC_VTABLE)
    }

    fn clone_panic_waker() -> Waker {
        // The raw pointer is never dereferenced; all operations are explicit no-ops
        // except clone, which intentionally panics for this regression.
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &CLONE_PANIC_VTABLE)) }
    }

    fn plain_waker() -> Waker {
        Waker::from(Arc::new(ThreadWake(std::thread::current())))
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = plain_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park_timeout(std::time::Duration::from_millis(1)),
            }
        }
    }

    fn test_owner(
        manager: &OperationManager,
        executor: &LocalIoExecutor,
        parent: &std::path::Path,
    ) -> LocalTempResource {
        let id = match manager
            .run(OperationOptions::default(), |scope| {
                let reservation = scope.reserve_resource().expect("identity reservation");
                let reference = match reservation.commit(()) {
                    Ok(reference) => reference,
                    Err(_) => panic!("identity resource commits"),
                };
                Ok::<_, ()>(reference.id())
            })
            .expect("identity operation")
        {
            OperationOutcome::Succeeded(id) => id,
            _ => panic!("identity operation succeeds"),
        };
        let file = LocalFileSystem::host()
            .expect("filesystem")
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
            .expect("native temp file");
        let owner = LocalTempResource::new(
            LocalTempNative::File(file),
            executor.reserve_disk(1).expect("disk permit"),
            executor.reserve_handle().expect("handle permit"),
            executor.reserve_cleanup().expect("cleanup permit"),
        );
        *owner.id.lock().expect("resource id lock") = Some(id);
        owner
    }

    fn cleanup_error() -> crate::LocalFileError {
        crate::LocalFileError::from_io(
            LocalFileOperation::Cleanup,
            None,
            None,
            std::io::Error::other("injected cleanup failure"),
        )
    }

    #[test]
    fn panicking_waker_clone_and_drop_do_not_lose_failed_cleanup_owner() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default())
            .expect("executor");
        let owner = test_owner(&manager, &executor, parent.path());
        let state = Arc::new(CleanupState {
            result: Mutex::new(None),
            waker: Mutex::new(None),
            owner: Mutex::new(Some(owner)),
        });
        let mut wait = Box::pin(CleanupWait {
            state: Arc::clone(&state),
            marker: std::marker::PhantomData,
        });

        let clone_panicking = clone_panic_waker();
        let mut clone_context = Context::from_waker(&clone_panicking);
        assert!(matches!(
            wait.as_mut().poll(&mut clone_context),
            Poll::Pending
        ));
        assert!(state.owner.lock().expect("owner lock").is_some());

        *state.waker.lock().expect("waker lock") =
            Some(unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &WAKE_PANIC_VTABLE)) });
        safe_wake(
            state
                .waker
                .lock()
                .expect("waker lock")
                .take()
                .expect("wake waker"),
        );

        let owner = state
            .owner
            .lock()
            .expect("owner lock")
            .take()
            .expect("owner retained after clone panic");
        *state.result.lock().expect("result lock") = Some(Some((cleanup_error(), owner)));
        *state.waker.lock().expect("waker lock") = Some(Waker::from(Arc::new(DropPanics)));
        let normal = plain_waker();
        let mut normal_context = Context::from_waker(&normal);
        let outcome = wait.as_mut().poll(&mut normal_context);
        let CleanupOutcome::Failed { resource, .. } = (match outcome {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("completed cleanup result remains observable"),
        }) else {
            panic!("simulated cleanup failure remains retryable");
        };

        let cleanup = Arc::new(LocalCleanup { executor });
        let context = CleanupContext::new(qubit_operation::ContextOptions::default())
            .expect("cleanup context");
        assert!(matches!(
            block_on(cleanup.cleanup(resource, context)),
            CleanupOutcome::Released
        ));
        assert_eq!(
            std::fs::read_dir(parent.path())
                .expect("read temp parent")
                .count(),
            0
        );
    }
}
