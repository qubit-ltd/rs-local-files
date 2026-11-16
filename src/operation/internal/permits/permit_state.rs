// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::atomic::AtomicUsize;

/// Tracks the bounded count shared by live permits.
pub(super) struct PermitState {
    /// Number of currently reserved units.
    pub(super) used: AtomicUsize,
    /// Maximum number of units that can be reserved.
    pub(super) limit: usize,
}
