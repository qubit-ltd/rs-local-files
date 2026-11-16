// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::super::LocalStreamRequest;

/// Associates one seek request with the writer that owns its completion.
/// The owner id prevents stale seek results from completing a later request.
pub(super) struct PendingSeek {
    /// Identifier of the writer whose poll owns this seek completion.
    pub(super) owner_id: u64,
    /// Native seek request awaiting worker completion.
    pub(super) request: LocalStreamRequest,
}
