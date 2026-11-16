use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use qubit_operation::{AsyncCleanup, CleanupContext, CleanupFuture, CleanupOutcome};

use super::LocalTempNative;
use super::LocalTempResource;
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
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
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
        let new_waker = cx.waker().clone();
        let old_waker = {
            let mut waker = self.state.waker.lock().unwrap_or_else(|e| e.into_inner());
            waker.replace(new_waker)
        };
        drop(old_waker);
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
    drop(waker);
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
            Some(LocalTempNative::Directory(value)) => value.cleanup().map(|()| {
                native.take();
                ()
            }),
            Some(LocalTempNative::Recovery(value)) => value.cleanup().map(|()| {
                native.take();
                ()
            }),
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
