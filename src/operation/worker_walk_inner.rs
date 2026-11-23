// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::any::Any;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::task::Poll;
use std::task::Waker;

use super::worker_walk_close_state::WorkerWalkCloseState;
use super::worker_walk_command::WorkerWalkCommand;
use super::worker_walk_cursor::WorkerWalkCursor;

/// Disposes of a panic payload without allowing its destructor to unwind.
///
/// If disposal panics, the resulting payload is also dropped under a panic
/// boundary; a further panicking payload is intentionally forgotten.
///
/// # Parameters
///
/// - `payload`: The panic payload to dispose of safely.
pub(crate) fn dispose_panic_payload(payload: Box<dyn Any + Send>) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
        && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
    {
        std::mem::forget(payload);
    }
}

/// Drops a close waiter outside its state lock and contains destructor panic.
///
/// # Parameters
///
/// - `waker`: The optional close waiter waker to drop safely.
pub(super) fn drop_close_waiter(waker: Option<Waker>) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker))) {
        dispose_panic_payload(payload);
    }
}

/// Test-only pause point bound to a request on one walker driver.
#[cfg(test)]
pub(super) type WalkBatchTransferTestGate = (u64, std::sync::mpsc::SyncSender<usize>, std::sync::mpsc::Receiver<()>);

/// Shared executor, iterator, and close state for one directory worker.
pub(super) struct WorkerWalkInner {
    /// Executor that serializes directory operations and cleanup.
    pub(super) executor: super::super::LocalIoExecutor,
    /// Native iterator and buffered entry guarded as one worker state.
    pub(super) walker: Mutex<WorkerWalkCursor>,
    /// Results keyed by stable caller request identifiers.
    pub(super) commands: Mutex<HashMap<u64, WorkerWalkCommand>>,
    /// Signals that no new directory entries should be read.
    pub(super) stop_requested: AtomicBool,
    /// Close result and all tasks waiting for its publication.
    pub(super) close_state: Arc<Mutex<WorkerWalkCloseState>>,
    /// Reserved capacity for worker-side cleanup.
    pub(super) cleanup_permit: Mutex<Option<super::super::internal::permits::Permit>>,
    /// Reservation for one bounded path batch until walker closure.
    pub(super) _buffer_budget: Arc<Mutex<super::super::internal::permits::Permit>>,
    /// Budget retained while the native directory handle is open.
    pub(super) _handle_budget: Mutex<Option<Arc<super::super::internal::permits::Permit>>>,
    /// One-shot panic injection scoped to this worker instance.
    #[cfg(test)]
    pub(super) panic_next_batch: std::sync::atomic::AtomicBool,
    /// Per-driver batch transfer gate used by deterministic worker tests.
    #[cfg(test)]
    pub(super) batch_transfer_test_gate: Mutex<Option<WalkBatchTransferTestGate>>,
    /// Per-driver event emitted immediately before cleanup acquires the cursor.
    #[cfg(test)]
    pub(super) cleanup_cursor_test_probe: Mutex<Option<std::sync::mpsc::SyncSender<bool>>>,
}

impl WorkerWalkInner {
    /// Registers a waiter atomically with checking the published close result.
    ///
    /// # Parameters
    /// - `waker` — task to notify when close reaches a terminal state.
    pub(super) fn poll_closed(&self, waker: &Waker) -> Poll<io::Result<()>> {
        if let Some(result) = self
            .close_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .result()
        {
            return Poll::Ready(result);
        }
        let registered_waker = match super::clone_poll_waker(waker) {
            Ok(waker) => waker,
            Err(payload) => {
                if let Some(result) = self
                    .close_state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .result()
                {
                    dispose_panic_payload(payload);
                    return Poll::Ready(result);
                }
                std::panic::resume_unwind(payload);
            }
        };
        let (result, unused_waker) = self
            .close_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .poll_legacy(registered_waker);
        drop_close_waiter(unused_waker);
        result
    }

    /// Publishes one terminal result, then notifies every waiter outside the
    /// lock.
    ///
    /// # Parameters
    /// - `result` — confirmed close or repeatable close failure to publish.
    pub(super) fn publish_close(&self, result: io::Result<()>) {
        let waiters = self
            .close_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .publish(result);
        let Some(waiters) = waiters else {
            return;
        };
        for waker in waiters {
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake())) {
                dispose_panic_payload(payload);
            }
        }
    }

    /// Releases the native handle reservation exactly once after iterator
    /// close.
    pub(super) fn release_handle_budget(&self) {
        self._handle_budget.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}
