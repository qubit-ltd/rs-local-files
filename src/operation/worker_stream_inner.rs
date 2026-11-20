// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use super::LocalNativeStream;
use super::worker_stream_queued_request::WorkerStreamQueuedRequest;
use super::worker_stream_state::WorkerStreamState;

/// Shared executor, native handle, and budgets for one stream worker.
pub(super) struct WorkerStreamInner {
    /// Executor that serializes native I/O and cleanup.
    pub(super) executor: super::super::LocalIoExecutor,
    /// Native reader or writer, retained until confirmed terminal cleanup.
    pub(super) native: Mutex<Option<LocalNativeStream>>,
    /// Results exposed to polling resource handles.
    pub(super) state: Mutex<WorkerStreamState>,
    /// FIFO commands waiting for native execution.
    pub(super) queue: Mutex<VecDeque<WorkerStreamQueuedRequest>>,
    /// Maps each parent write and payload offset to one worker request ID.
    pub(super) native_write_ids: Mutex<std::collections::HashMap<(u64, usize), u64>>,
    /// Next ID reserved for an internal native write attempt.
    pub(super) next_internal_request_id: std::sync::atomic::AtomicU64,
    /// Whether a drain task currently owns the queue.
    pub(super) active: AtomicBool,
    /// Reserved capacity for worker-side cleanup.
    pub(super) cleanup_permit: Mutex<Option<super::super::internal::permits::Permit>>,
    /// Budget retained while buffered stream data is in use.
    pub(super) _buffer_budget: Arc<Mutex<super::super::internal::permits::Permit>>,
    /// Budget retained while the native stream handle is open.
    pub(super) _handle_budget: Mutex<Option<Arc<super::super::internal::permits::Permit>>>,
    /// Optional effect observer updated after each successful append write.
    pub(super) observer: Option<super::super::LocalFactObserver>,
    /// Total bytes confirmed written through this worker.
    pub(super) written_bytes: std::sync::atomic::AtomicU64,
    /// Whether successful writes publish append-specific facts.
    pub(super) append_mode: bool,
}

impl WorkerStreamInner {
    /// Returns the native handle reservation after confirmed stream closure.
    pub(super) fn release_handle_budget(&self) {
        self._handle_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }
}
