// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::atomic::AtomicU64;

/// Tracks temporary disk capacity shared by disk permits.
#[allow(dead_code)]
pub(super) struct DiskPermitState {
    /// Number of temporary bytes currently reserved.
    pub(super) used: AtomicU64,
    /// Maximum temporary bytes available to the pool.
    pub(super) limit: u64,
}
