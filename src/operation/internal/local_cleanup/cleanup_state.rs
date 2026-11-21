// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Mutex;
use std::task::Waker;

use crate::operation::temp::LocalTempResource;

/// Shared handoff state between the cleanup worker and its waiting future.
///
/// `owner` remains populated until the worker takes responsibility; `result`
/// publishes either successful release or the still-owned resource on error.
pub(super) struct CleanupState {
    /// Worker result; the outer option distinguishes “not finished” from
    /// finish.
    pub(super) result: Mutex<Option<Option<(crate::LocalFileError, LocalTempResource)>>>,
    /// Worker result awaiting release of its cleanup queue permit.
    pub(super) pending_result: Mutex<Option<Option<(crate::LocalFileError, LocalTempResource)>>>,
    /// Most recently registered task waker, if the future is being polled.
    pub(super) waker: Mutex<Option<Waker>>,
    /// Resource not yet taken by the cleanup worker.
    pub(super) owner: Mutex<Option<LocalTempResource>>,
}
