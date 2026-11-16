// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::AsyncResult;
use super::safe_wake;

/// Completes one result cell and wakes its waiting receiver.
///
/// # Type Parameters
///
/// * `T` — result value delivered to the receiver.
pub(super) struct AsyncSender<T>(
    /// Shared result cell completed by this one-shot sender.
    pub(super) Arc<AsyncResult<T>>,
);
impl<T> AsyncSender<T> {
    /// Publishes the value and notifies the receiver, if it has been polled.
    ///
    /// # Parameters
    ///
    /// * `self` — one-shot sender consumed by completion.
    /// * `value` — result value transferred to the waiting receiver.
    pub(super) fn send(self, value: T) -> Result<(), T> {
        *self.0.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
        let waker = self.0.waker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(waker) = waker {
            safe_wake(waker);
        }
        Ok(())
    }
}
