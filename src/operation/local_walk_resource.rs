// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use super::LOCAL_WALK_BATCH_BYTES;
use super::LOCAL_WALK_BATCH_ENTRIES;
use super::LocalWalkDriver;
use super::close_wait_registration::CloseWaitRegistration;
use super::dispose_panic_payload;
use super::worker_walk_close_state::WorkerWalkCloseState;

/// Managed control port for an exclusively-owned directory walker.
///
/// The operation scope owns cleanup for the native iterator while walker
/// handles share this single control port.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalWalkDriver;
/// use qubit_local_files::operation::stream::LocalWalkResource;
/// use std::io;
/// use std::path::PathBuf;
/// use std::task::Context;
/// use std::task::Poll;
///
/// struct EmptyWalker;
///
/// impl LocalWalkDriver for EmptyWalker {
///     fn poll_batch(
///         &self,
///         _: &mut Context<'_>,
///         _: u64,
///         _: usize,
///         _: usize,
///     ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
///         Poll::Ready(Ok(None))
///     }
///
///     fn request_stop(&self) {}
///
///     fn poll_closed(&self, _: &mut Context<'_>) -> Poll<io::Result<()>> {
///         Poll::Ready(Ok(()))
///     }
/// }
///
/// let port = LocalWalkResource::new(EmptyWalker);
/// drop(port);
/// ```
pub struct LocalWalkResource {
    /// Worker adapter that owns the native directory iterator.
    driver: Arc<dyn LocalWalkDriver>,
    /// Prevents concurrent walkers from sharing one iterator.
    claimed: AtomicBool,
    /// Resource identifier installed when the port enters an operation scope.
    pub(super) resource_id: Arc<Mutex<Option<qubit_operation::ResourceId>>>,
    /// Native close wait registry; custom drivers keep their own notification
    /// policy.
    close_state: Option<Arc<Mutex<WorkerWalkCloseState>>>,
}

#[cfg(all(test, panic = "unwind"))]
mod tests {
    use std::io;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;

    use super::super::LocalWalkDriver;
    use super::LocalWalkResource;

    struct PanicOnDrop(Arc<AtomicUsize>);

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("custom walker panic payload destructor panicked");
        }
    }

    struct PanickingDriver(Arc<AtomicUsize>);

    impl LocalWalkDriver for PanickingDriver {
        fn poll_batch(
            &self,
            _: &mut Context<'_>,
            _: u64,
            _: usize,
            _: usize,
        ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
            std::panic::panic_any(PanicOnDrop(Arc::clone(&self.0)));
        }

        fn request_stop(&self) {
            std::panic::panic_any(PanicOnDrop(Arc::clone(&self.0)));
        }

        fn poll_closed(&self, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            std::panic::panic_any(PanicOnDrop(Arc::clone(&self.0)));
        }
    }

    #[test]
    fn test_custom_driver_batch_stop_and_close_payloads_are_disposed() {
        let drops = Arc::new(AtomicUsize::new(0));
        let resource = LocalWalkResource::new(PanickingDriver(Arc::clone(&drops)));
        let mut context = Context::from_waker(std::task::Waker::noop());

        let batch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| resource.poll_batch(&mut context, 1)))
            .expect("captured batch panic payload is safely disposed");
        assert!(matches!(batch, Poll::Ready(Err(error)) if error.to_string().contains("Unconfirmed")));

        let closed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            resource.poll_closed(&mut context, None)
        }))
        .expect("captured close panic payload is safely disposed");
        assert!(matches!(closed, Poll::Ready(Err(error)) if error.to_string().contains("Unconfirmed")));

        let stop = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| resource.request_stop()));
        assert!(stop.is_ok(), "stop panic payload is safely disposed");
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }
}

impl LocalWalkResource {
    /// Wraps a serial-worker walker port.
    ///
    /// # Parameters
    ///
    /// * `driver` - Worker adapter that owns the native directory iterator.
    pub fn new(driver: impl LocalWalkDriver) -> Self {
        Self {
            driver: Arc::new(driver),
            claimed: AtomicBool::new(false),
            resource_id: Arc::new(Mutex::new(None)),
            close_state: None,
        }
    }

    /// Wraps the native worker with its shared cancellable close state.
    ///
    /// # Parameters
    /// - `driver` — worker adapter that owns the native directory iterator.
    /// - `close_state` — terminal state shared by native close waiters.
    pub(super) fn new_native(driver: impl LocalWalkDriver, close_state: Arc<Mutex<WorkerWalkCloseState>>) -> Self {
        let mut resource = Self::new(driver);
        resource.close_state = Some(close_state);
        resource
    }

    /// Creates an independent close waiter for native workers.
    pub(super) fn close_registration(&self) -> Option<CloseWaitRegistration> {
        self.close_state
            .as_ref()
            .map(|state| CloseWaitRegistration::new(Arc::clone(state)))
    }

    /// Reserves the walker port for one exclusive traversal handle.
    pub(super) fn claim(&self) -> io::Result<()> {
        self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| io::Error::new(io::ErrorKind::AlreadyExists, "walker already has an exclusive handle"))
    }

    /// Polls a bounded batch while converting driver panics to an I/O error.
    ///
    /// # Parameters
    ///
    /// * `cx` - Caller task context used by the worker driver for wakeups.
    /// * `request_id` - Stable identifier reused when polling the same batch.
    pub(super) fn poll_batch(&self, cx: &mut Context<'_>, request_id: u64) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.driver
                .poll_batch(cx, request_id, LOCAL_WALK_BATCH_ENTRIES, LOCAL_WALK_BATCH_BYTES)
        })) {
            Ok(result) => result,
            Err(payload) => {
                dispose_panic_payload(payload);
                Poll::Ready(Err(io::Error::other("Unconfirmed: local walker driver panicked")))
            }
        }
    }

    /// Enqueues worker-side stop and contains driver panics.
    pub(super) fn request_stop(&self) {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.request_stop())) {
            dispose_panic_payload(payload);
        }
    }

    /// Polls for native iterator closure while converting driver panics.
    ///
    /// # Parameters
    ///
    /// * `cx` - Caller task context used by the worker driver for wakeups.
    pub(super) fn poll_closed(
        &self,
        cx: &mut Context<'_>,
        registration: Option<&mut CloseWaitRegistration>,
    ) -> Poll<io::Result<()>> {
        if self.close_state.is_some()
            && let Some(registration) = registration
        {
            return registration.poll(cx.waker());
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.poll_closed(cx))) {
            Ok(result) => result,
            Err(payload) => {
                dispose_panic_payload(payload);
                Poll::Ready(Err(io::Error::other("Unconfirmed: local walker close driver panicked")))
            }
        }
    }
}
