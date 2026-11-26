// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Captured options for a worker-backed local stream.

/// Optional state captured when a worker-backed stream is registered.
pub(crate) struct WorkerStreamOptions {
    /// Receives facts produced by writes, when configured.
    pub(crate) observer: Option<super::super::LocalFactObserver>,
    /// Whether writes should update append-specific facts.
    pub(crate) append_mode: bool,
    /// Identity of the filesystem authority that opened the stream.
    pub(crate) authority_identity: usize,
}
