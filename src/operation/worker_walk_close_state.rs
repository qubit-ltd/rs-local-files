// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::task::Poll;
use std::task::Waker;

/// Shared terminal result and waiters for a worker-backed directory walk.
#[derive(Default)]
pub(super) struct WorkerWalkCloseState {
    /// Published close result; `None` means cleanup is still pending.
    terminal: Option<Result<(), (io::ErrorKind, String)>>,
    /// Independent tasks waiting for terminal close publication.
    waiters: Vec<Waker>,
}

impl WorkerWalkCloseState {
    /// Returns the close result or registers one independent waiter.
    ///
    /// # Parameters
    /// - `waker` — task to notify when close reaches a terminal state.
    pub(super) fn poll(&mut self, waker: &Waker) -> Poll<io::Result<()>> {
        if let Some(result) = &self.terminal {
            return match result {
                Ok(()) => Poll::Ready(Ok(())),
                Err((kind, message)) => Poll::Ready(Err(io::Error::new(*kind, message.clone()))),
            };
        }
        if !self.waiters.iter().any(|registered| registered.will_wake(waker)) {
            self.waiters.push(waker.clone());
        }
        Poll::Pending
    }

    /// Publishes a terminal result once and returns all registered waiters.
    ///
    /// # Parameters
    /// - `result` — confirmed close or repeatable close failure to publish.
    pub(super) fn publish(&mut self, result: io::Result<()>) -> Option<Vec<Waker>> {
        if self.terminal.is_some() {
            return None;
        }
        self.terminal = Some(match result {
            Ok(()) => Ok(()),
            Err(error) => Err((error.kind(), error.to_string())),
        });
        Some(std::mem::take(&mut self.waiters))
    }
}
