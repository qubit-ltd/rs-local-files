// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by delete integration tests.

/// Options for deleting a native filesystem entry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "delete options have no effect unless they are used"]
pub struct LocalDeleteOptions {
    /// Whether directory descendants should be removed.
    recursive: bool,
    /// Whether a missing entry is treated as a successful no-op.
    missing_ok: bool,
}

impl LocalDeleteOptions {
    /// Creates strict, non-recursive deletion options.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
    pub const fn new() -> Self {
        Self {
            recursive: false,
            missing_ok: false,
        }
    }

    /// Reports whether directory deletion is recursive.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Reports whether missing entries are accepted.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn missing_ok(&self) -> bool {
        self.missing_ok
    }

    /// Enables recursive directory deletion.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Treats a missing entry as a successful no-op.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_missing_ok(mut self) -> Self {
        self.missing_ok = true;
        self
    }
}
