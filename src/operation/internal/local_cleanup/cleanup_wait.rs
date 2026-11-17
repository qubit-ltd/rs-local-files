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

use super::CleanupState;
use super::cleanup_outcome;
use super::clear_waker;
use super::safe_clone_waker;
use super::safe_drop_waker;
use crate::operation::temp::LocalTempResource;

/// Future that collects the result from one queued native cleanup job.
pub(crate) struct CleanupWait {
    /// State shared with the worker closure for this cleanup request.
    pub(super) state: Arc<CleanupState>,
    /// Associates the wait future with the resource type being cleaned.
    pub(super) marker: std::marker::PhantomData<LocalTempResource>,
}

impl Future for CleanupWait {
    /// Cleanup status with the resource returned on failure.
    type Output = CleanupOutcome<LocalTempResource>;

    /// Returns the worker result or stores the current task waker until it
    /// arrives.
    ///
    /// # Parameters
    ///
    /// * `self` — pinned wait future sharing state with the cleanup worker.
    /// * `cx` — task context used to arrange a wake when cleanup completes.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = self.state.result.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(result) = result {
            clear_waker(&self.state);
            return Poll::Ready(cleanup_outcome(result));
        }
        let Some(new_waker) = safe_clone_waker(cx.waker()) else {
            let result = self.state.result.lock().unwrap_or_else(|e| e.into_inner()).take();
            return match result {
                Some(result) => {
                    clear_waker(&self.state);
                    Poll::Ready(cleanup_outcome(result))
                }
                None => Poll::Pending,
            };
        };
        let old_waker = {
            let mut waker = self.state.waker.lock().unwrap_or_else(|e| e.into_inner());
            waker.replace(new_waker)
        };
        safe_drop_waker(old_waker);
        let result = self.state.result.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(result) = result {
            clear_waker(&self.state);
            Poll::Ready(cleanup_outcome(result))
        } else {
            Poll::Pending
        }
    }
}
