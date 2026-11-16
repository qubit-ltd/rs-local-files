// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::super::handoff::LocalReadLease;

/// Successful result of one temporary lifecycle transition.
pub enum LocalTempLifecycleResult {
    /// The temporary resource was published outside its private sandbox.
    Persisted(crate::outcome::LocalPersistOutcome),
    /// The same managed resource now contains a sealed read-only file.
    Sealed(LocalReadLease),
}
