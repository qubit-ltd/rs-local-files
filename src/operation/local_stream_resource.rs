// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use super::LocalStreamCompletion;
use super::LocalStreamDriver;
use super::LocalStreamRequest;
use super::dispose_panic_payload;
use super::native_stream_writer::NativeStreamWriter;

/// Thread-safe control port for one exclusively-owned native stream.
///
/// Cloning a `ResourceRef` shares this control port; it does not create a
/// second mutable reader or writer. `ResourceRef` keeps its owning operation
/// resource alive while an async stream handle exists.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalStreamCompletion;
/// use qubit_local_files::operation::stream::LocalStreamDriver;
/// use qubit_local_files::operation::stream::LocalStreamRequest;
/// use qubit_local_files::operation::stream::LocalStreamResource;
/// use std::io;
/// use std::task::Context;
/// use std::task::Poll;
///
/// struct PendingDriver;
///
/// impl LocalStreamDriver for PendingDriver {
///     fn poll_command(
///         &self,
///         _: &mut Context<'_>,
///         _: &LocalStreamRequest,
///     ) -> Poll<io::Result<LocalStreamCompletion>> {
///         Poll::Pending
///     }
///
///     fn request_close(&self, _: u64) {}
/// }
///
/// let port = LocalStreamResource::new(PendingDriver);
/// drop(port);
/// ```
pub struct LocalStreamResource {
    /// Driver that marshals owned requests onto the native worker.
    driver: Arc<dyn LocalStreamDriver>,
    /// Private route for native writes that retain the original payload.
    native_writer: Option<Arc<dyn NativeStreamWriter>>,
    /// Prevents two handles from mutating the same native stream.
    claimed: AtomicBool,
    /// Resource identifier installed when the port enters an operation scope.
    pub(super) resource_id: Arc<Mutex<Option<qubit_operation::ResourceId>>>,
}

impl LocalStreamResource {
    /// Wraps a serial-worker control port.
    ///
    /// # Parameters
    ///
    /// * `driver` - Worker adapter that owns and executes native stream
    ///   requests.
    pub fn new(driver: impl LocalStreamDriver) -> Self {
        Self {
            driver: Arc::new(driver),
            native_writer: None,
            claimed: AtomicBool::new(false),
            resource_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Wraps a worker driver with the internal native write route.
    ///
    /// # Type Parameters
    ///
    /// * `T` - Worker adapter implementing the public and native write routes.
    ///
    /// # Parameters
    ///
    /// * `driver` - Worker adapter that supports native payload-offset writes.
    pub(super) fn new_native<T>(driver: T) -> Self
    where
        T: LocalStreamDriver + NativeStreamWriter + 'static,
    {
        let driver = Arc::new(driver);
        let native_writer: Arc<dyn NativeStreamWriter> = driver.clone();
        Self {
            driver,
            native_writer: Some(native_writer),
            claimed: AtomicBool::new(false),
            resource_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Reserves the port for one exclusive reader or writer handle.
    pub(super) fn claim(&self) -> io::Result<()> {
        self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "local stream already has an exclusive handle",
                )
            })
    }

    /// Polls the driver while converting driver panics to an I/O error.
    ///
    /// # Parameters
    ///
    /// * `cx` - Caller task context used by the worker driver for wakeups.
    /// * `request` - Stable command request to poll.
    pub(super) fn poll_command(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
    ) -> Poll<io::Result<LocalStreamCompletion>> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.poll_command(cx, request))) {
            Ok(result) => result,
            Err(payload) => {
                dispose_panic_payload(payload);
                Poll::Ready(Err(io::Error::other("Unconfirmed: local stream driver panicked")))
            }
        }
    }

    /// Returns write facts established by the underlying worker, if available.
    pub(super) fn write_facts(&self) -> Option<(crate::LocalWriteFailureState, u64)> {
        self.driver.write_facts()
    }

    /// Polls a native payload-offset write when this resource supports it.
    ///
    /// # Parameters
    ///
    /// * `cx` - Poll context used to register the task waker.
    /// * `request` - Original write request whose payload remains shared.
    /// * `offset` - Number of payload bytes already written.
    pub(super) fn poll_native_write(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
        offset: usize,
    ) -> Option<Poll<io::Result<LocalStreamCompletion>>> {
        self.native_writer
            .as_ref()
            .map(|writer| writer.poll_native_write(cx, request, offset))
    }

    /// Requests an idempotent close while containing driver panics.
    ///
    /// # Parameters
    ///
    /// * `request_id` - Stable identifier assigned to the close request.
    pub(super) fn request_close(&self, request_id: u64) {
        if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.request_close(request_id)))
        {
            dispose_panic_payload(payload);
        }
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

    use super::super::LocalStreamCompletion;
    use super::super::LocalStreamDriver;
    use super::super::LocalStreamRequest;
    use super::LocalStreamResource;

    struct CustomDriver;

    impl LocalStreamDriver for CustomDriver {
        fn poll_command(&self, _: &mut Context<'_>, _: &LocalStreamRequest) -> Poll<io::Result<LocalStreamCompletion>> {
            Poll::Pending
        }

        fn request_close(&self, _: u64) {}
    }

    #[test]
    fn test_custom_driver_has_no_worker_confirmed_write_facts() {
        let resource = LocalStreamResource::new(CustomDriver);
        let read_facts: fn(&LocalStreamResource) -> Option<(crate::LocalWriteFailureState, u64)> =
            LocalStreamResource::write_facts;
        assert_eq!(read_facts(&resource), None);
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_custom_driver_panic_payloads_are_disposed_inside_capture_boundary() {
        struct PanicOnDrop(Arc<AtomicUsize>);

        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("custom stream panic payload destructor panicked");
            }
        }

        struct PanickingDriver(Arc<AtomicUsize>);

        impl LocalStreamDriver for PanickingDriver {
            fn poll_command(
                &self,
                _: &mut Context<'_>,
                _: &LocalStreamRequest,
            ) -> Poll<io::Result<LocalStreamCompletion>> {
                std::panic::panic_any(PanicOnDrop(Arc::clone(&self.0)));
            }

            fn request_close(&self, _: u64) {
                std::panic::panic_any(PanicOnDrop(Arc::clone(&self.0)));
            }
        }

        let drops = Arc::new(AtomicUsize::new(0));
        let resource = LocalStreamResource::new(PanickingDriver(Arc::clone(&drops)));
        let request = LocalStreamRequest {
            id: 1,
            command: super::super::LocalStreamCommand::Close,
        };
        let mut context = Context::from_waker(std::task::Waker::noop());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            resource.poll_command(&mut context, &request)
        }))
        .expect("captured command panic payload is safely disposed");
        assert!(matches!(result, Poll::Ready(Err(error)) if error.to_string().contains("Unconfirmed")));

        let close = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| resource.request_close(1)));
        assert!(close.is_ok(), "close panic payload is safely disposed");
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }
}
