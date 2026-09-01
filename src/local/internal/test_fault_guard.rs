// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scoped test fault guard for deterministic fault injection.

/// Releases one process-local deterministic fault selector when dropped.
#[derive(Debug)]
#[cfg(feature = "internal-test-support")]
#[doc(hidden)]
pub struct TestFaultGuard {
    /// Whether this guard still owns the active selector.
    pub(crate) active: bool,
}
