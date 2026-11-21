// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Mutex;
use std::task::Waker;

use qubit_operation::CleanupOutcome;

use super::LocalManagedAbortResource;

/// Shared owner, completion, and wake state for an async abort cleanup.
///
/// # Type Parameters
///
/// * `W` - Native writer type retained while abort work is pending.
pub(in crate::operation) struct LocalAbortCleanupState<W: Send + 'static> {
    /// Writer resource moved to the worker when cleanup starts.
    pub(in crate::operation) owner: Mutex<Option<LocalManagedAbortResource<W>>>,
    /// Cleanup result returned to the operation scope.
    pub(in crate::operation) result: Mutex<Option<CleanupOutcome<LocalManagedAbortResource<W>>>>,
    /// Worker result awaiting release of its cleanup queue permit.
    pub(in crate::operation) pending_result: Mutex<Option<CleanupOutcome<LocalManagedAbortResource<W>>>>,
    /// Task to wake after the worker publishes the cleanup result.
    pub(in crate::operation) waker: Mutex<Option<Waker>>,
}
