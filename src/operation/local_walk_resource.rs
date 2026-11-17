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
        }
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
            Err(_) => Poll::Ready(Err(io::Error::other("Unconfirmed: local walker driver panicked"))),
        }
    }

    /// Enqueues worker-side stop and contains driver panics.
    pub(super) fn request_stop(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.request_stop()));
    }

    /// Polls for native iterator closure while converting driver panics.
    ///
    /// # Parameters
    ///
    /// * `cx` - Caller task context used by the worker driver for wakeups.
    pub(super) fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.poll_closed(cx))) {
            Ok(result) => result,
            Err(_) => Poll::Ready(Err(io::Error::other("Unconfirmed: local walker close driver panicked"))),
        }
    }
}
