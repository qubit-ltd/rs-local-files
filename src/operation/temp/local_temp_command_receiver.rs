// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use super::AsyncReceiver;
use super::LocalTempCommandError;

/// Future receiving a local worker command result without doing native I/O in
/// poll.
///
/// # Type Parameters
///
/// * `T` — command result returned when the worker finishes.
pub(crate) struct LocalTempCommandReceiver<T>(
    /// Completion future for the serialized native command.
    AsyncReceiver<Result<T, LocalTempCommandError>>,
);

impl<T> Future for LocalTempCommandReceiver<T> {
    /// Command result or the scheduling/native failure that prevented it.
    type Output = Result<T, LocalTempCommandError>;

    /// Polls the executor result receiver and propagates its command outcome.
    ///
    /// # Parameters
    ///
    /// * `self` — receiver future waiting for one serialized native command.
    /// * `cx` — task context used to register for worker completion.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}
