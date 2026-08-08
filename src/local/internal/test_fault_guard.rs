// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scoped test fault guard for deterministic fault injection.

#[derive(Debug)]
#[cfg(feature = "internal-test-support")]
#[doc(hidden)]
pub struct TestFaultGuard {
    pub(crate) active: bool,
}
