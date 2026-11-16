// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::super::LocalStreamRequest;

/// Retains a write request and its progress while the worker writes in chunks.
/// The offset prevents already-written bytes from being submitted twice.
pub(super) struct PendingWrite {
    /// Native write request whose bytes have not all been accepted yet.
    pub(super) request: LocalStreamRequest,
    /// Number of leading request bytes already written successfully.
    pub(super) offset: usize,
}
