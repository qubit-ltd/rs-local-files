// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by directory integration tests.

/// Options for creating a native directory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalCreateDirectoryOptions {
    /// Whether missing ancestors should also be created.
    recursive: bool,
}

impl LocalCreateDirectoryOptions {
    /// Creates options for creating exactly one directory entry.
    #[must_use]
    #[inline(always)]
    pub const fn new() -> Self {
        Self { recursive: false }
    }

    /// Reports whether missing ancestors are created.
    #[must_use]
    #[inline(always)]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Enables recursive ancestor creation.
    #[must_use]
    #[inline(always)]
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }
}
