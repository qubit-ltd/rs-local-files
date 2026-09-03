// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Process-local test fault selector state.

/// Selector owner and name retained while one test controls fault injection.
#[cfg(feature = "test-support")]
pub(super) struct ActiveFault {
    /// Test thread that owns the currently installed selector.
    pub(super) owner: std::thread::ThreadId,
    /// Native fault boundary selected by the owning test.
    pub(super) name: String,
}
