// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scoped test fault guard for deterministic fault injection.

/// Releases one process-local deterministic fault selector when dropped.
#[doc(hidden)]
#[derive(Debug)]
#[must_use = "the guard must remain alive while the test fault is active"]
#[cfg(feature = "test-support")]
pub struct TestFaultGuard {
    /// Whether this guard still owns the active selector.
    pub(crate) active: bool,
}

#[cfg(feature = "test-support")]
impl Drop for TestFaultGuard {
    /// Releases the process-wide selector and occurrence counters.
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        super::test_support::release_test_fault();
        self.active = false;
    }
}
