// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;
use std::task::Waker;

use super::AsyncReceiver;
use super::AsyncSender;

/// Shared one-shot completion cell for a native worker command.
/// The worker publishes one value and may take the stored waker to notify the
/// task waiting for that result.
///
/// # Type Parameters
///
/// * `T` — result value shared by the worker and receiver.
pub(crate) struct AsyncResult<T> {
    /// Result value published by the worker and consumed by the receiver.
    pub(super) value: Mutex<Option<T>>,
    /// Current receiver task waker, if the future is pending.
    pub(super) waker: Mutex<Option<Waker>>,
}

/// Creates a sender and receiver backed by one completion cell.
///
/// # Type Parameters
///
/// * `T` — result delivered once by the worker.
pub(super) fn async_result<T: Send + 'static>() -> (AsyncSender<T>, AsyncReceiver<T>) {
    let state = Arc::new(AsyncResult {
        value: Mutex::new(None),
        waker: Mutex::new(None),
    });
    (AsyncSender(Arc::clone(&state)), AsyncReceiver(state))
}
