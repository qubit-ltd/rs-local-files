// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::marker::PhantomData;

use super::LocalIoExecutor;

/// Cleanup adapter that runs native writer aborts on the serial executor.
///
/// # Type Parameters
///
/// * `W` - Native writer type retained by the cleanup resource.
pub(in crate::operation) struct LocalManagedAbortCleanup<W: Send + 'static> {
    /// Executor that serializes native abort work.
    pub(in crate::operation) executor: LocalIoExecutor,
    /// Associates this policy with its writer type without storing a writer.
    pub(in crate::operation) marker: PhantomData<fn(W)>,
}

impl<W: Send + 'static> LocalManagedAbortCleanup<W> {
    /// Creates a cleanup policy bound to the supplied serial executor.
    ///
    /// # Parameters
    ///
    /// * `executor` - Executor used to queue native abort work.
    pub(in crate::operation) fn new(executor: LocalIoExecutor) -> Self {
        Self {
            executor,
            marker: PhantomData,
        }
    }
}
