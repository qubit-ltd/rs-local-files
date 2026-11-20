// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalStreamRequest;

/// A queued worker request with its native payload progress offset.
pub(super) struct WorkerStreamQueuedRequest {
    /// Original request retained across native short writes.
    pub(super) request: LocalStreamRequest,
    /// Number of bytes already written from a native write payload.
    pub(super) write_offset: usize,
}
