// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use super::AsyncResult;
use super::safe_clone_waker;
use super::safe_drop_waker;

/// Future that consumes the one-shot result published by a worker.
///
/// # Type Parameters
///
/// * `T` — result returned when the worker completes.
pub(super) struct AsyncReceiver<T>(
    /// Shared result cell polled until the worker publishes a value.
    pub(super) Arc<AsyncResult<T>>,
);
impl<T> Future for AsyncReceiver<T> {
    /// One result value published by the worker.
    type Output = T;
    /// Takes a completed result or registers this task for worker notification.
    ///
    /// # Parameters
    ///
    /// * `self` — pinned receiver future that owns the completion cell.
    /// * `cx` — task context used to register a wakeup.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        if let Some(value) = self.0.value.lock().unwrap_or_else(|e| e.into_inner()).take() {
            return Poll::Ready(value);
        }
        let Some(new_waker) = safe_clone_waker(cx.waker()) else {
            return self
                .0
                .value
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .map_or(Poll::Pending, Poll::Ready);
        };
        let old_waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(new_waker);
        safe_drop_waker(old_waker);
        if let Some(value) = self.0.value.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Poll::Ready(value)
        } else {
            Poll::Pending
        }
    }
}
