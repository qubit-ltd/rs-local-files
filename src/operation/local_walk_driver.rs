// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::PathBuf;
use std::task::Context;
use std::task::Poll;

/// Serial worker port for a lazy directory walker.
///
/// Drivers retain stable batch results until their request is polled to
/// completion, allowing the handle to settle cancellation without advancing
/// the native iterator twice.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalWalkDriver;
/// use qubit_local_files::operation::stream::LocalWalkResource;
///
/// fn make_port<D: LocalWalkDriver>(driver: D) -> LocalWalkResource {
///     LocalWalkResource::new(driver)
/// }
/// ```
pub trait LocalWalkDriver: Send + Sync + 'static {
    /// Polls one stable batch request. Re-polling the same id must observe the
    /// same result; a driver must not advance its native walker twice.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context to wake when the requested batch is available.
    /// * `request_id` - Stable identifier reused when polling is repeated.
    /// * `max_entries` - Maximum path count the caller can retain in one batch.
    /// * `max_bytes` - Maximum encoded path bytes the caller can retain.
    fn poll_batch(
        &self,
        cx: &mut Context<'_>,
        request_id: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>>;

    /// Requests worker-side stop and native directory handle closure.
    /// Implementations must enqueue/merge this without blocking the caller.
    fn request_stop(&self);

    /// Confirms the native directory handle is closed.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context to wake when native closure is confirmed. Drivers
    ///   must retain and notify every independent active waiter, including
    ///   observers and callers of `close`, when the shared result is published.
    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}
