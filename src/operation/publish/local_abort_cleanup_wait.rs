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

use qubit_operation::CleanupOutcome;

use super::LocalAbortCleanupState;
use super::LocalManagedAbortResource;

/// Future that returns the worker-confirmed abort cleanup outcome.
///
/// # Type Parameters
///
/// * `W` - Native writer type owned by the pending cleanup.
pub(in crate::operation) struct LocalAbortCleanupWait<W: Send + 'static>(
    /// Shared state used to observe worker completion and register the waker.
    pub(in crate::operation) Arc<LocalAbortCleanupState<W>>,
);

impl<W: Send + 'static> Future for LocalAbortCleanupWait<W> {
    /// Cleanup outcome returned when the worker finishes aborting the writer.
    type Output = CleanupOutcome<LocalManagedAbortResource<W>>;

    /// Polls for the worker result and registers the current task for wakeup.
    ///
    /// # Parameters
    ///
    /// * `self` - Pending cleanup wait future.
    /// * `context` - Task context used to install the completion waker.
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self
            .0
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            return Poll::Ready(result);
        }
        let Some(new_waker) = super::super::temp::safe_clone_waker(context.waker()) else {
            return Poll::Pending;
        };
        let old_waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(new_waker);
        super::super::temp::safe_drop_waker(old_waker);
        match self
            .0
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending,
        }
    }
}
