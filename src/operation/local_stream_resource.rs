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
            Err(_) => Poll::Ready(Err(io::Error::other("Unconfirmed: local stream driver panicked"))),
        }
    }

    /// Returns write facts established by the underlying worker, if available.
    pub(super) fn write_facts(&self) -> Option<(crate::LocalWriteFailureState, u64)> {
        self.driver.write_facts()
    }

    /// Requests an idempotent close while containing driver panics.
    ///
    /// # Parameters
    ///
    /// * `request_id` - Stable identifier assigned to the close request.
    pub(super) fn request_close(&self, request_id: u64) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.request_close(request_id)));
    }
}
