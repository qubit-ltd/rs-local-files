// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::task::Waker;

use super::worker_walk_command::WorkerWalkCommand;
use super::worker_walk_cursor::WorkerWalkCursor;

/// Shared executor, iterator, and close state for one directory worker.
pub(super) struct WorkerWalkInner {
    /// Executor that serializes directory operations and cleanup.
    pub(super) executor: super::super::LocalIoExecutor,
    /// Native iterator and buffered entry guarded as one worker state.
    pub(super) walker: Mutex<WorkerWalkCursor>,
    /// Results keyed by stable caller request identifiers.
    pub(super) commands: Mutex<HashMap<u64, WorkerWalkCommand>>,
    /// Signals that no new directory entries should be read.
    pub(super) stop_requested: AtomicBool,
    /// Confirms the native iterator has been dropped.
    pub(super) closed: AtomicBool,
    /// Captured error from closing the native iterator.
    pub(super) close_error: Mutex<Option<(io::ErrorKind, String)>>,
    /// Task waiting for native close confirmation.
    pub(super) close_waker: Mutex<Option<Waker>>,
    /// Reserved capacity for worker-side cleanup.
    pub(super) cleanup_permit: Mutex<Option<super::super::internal::permits::Permit>>,
    /// Budget retained while returned path batches remain alive.
    pub(super) _buffer_budget: Arc<Mutex<super::super::internal::permits::Permit>>,
    /// Budget retained while the native directory handle is open.
    pub(super) _handle_budget: Arc<super::super::internal::permits::Permit>,
}
