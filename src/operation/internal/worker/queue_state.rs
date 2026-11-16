// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Mutable state for the ordinary and cleanup task queues.

use std::collections::VecDeque;

/// Tracks queued jobs and the worker's fair scheduling state.
pub(super) struct QueueState {
    /// Bounded jobs that perform ordinary local operations.
    pub(super) ordinary: VecDeque<Box<dyn FnOnce() + Send + 'static>>,
    /// Bounded jobs reserved for resource cleanup.
    pub(super) cleanup: VecDeque<Box<dyn FnOnce() + Send + 'static>>,
    /// Whether the worker should exit after both queues drain.
    pub(super) stopping: bool,
    /// Selects the queue opposite the preceding task when both have work.
    pub(super) last_was_cleanup: bool,
}
