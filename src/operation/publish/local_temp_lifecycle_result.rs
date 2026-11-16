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
    /// The temporary resource was moved to a caller-visible destination and the outcome describes the publication guarantees.
    Persisted(crate::outcome::LocalPersistOutcome),
    /// Sealing succeeded and returns a read lease for the resource now protected against writes.
    Sealed(LocalReadLease),
}
