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
#[must_use = "directory creation options have no effect unless they are used"]
pub struct LocalCreateDirectoryOptions {
    /// Whether missing ancestors should also be created.
    recursive: bool,
    /// Whether an existing directory is accepted as a successful outcome.
    exists_ok: bool,
}

impl LocalCreateDirectoryOptions {
    /// Creates options for creating exactly one directory entry.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            recursive: false,
            exists_ok: false,
        }
    }

    /// Reports whether missing ancestors are created.
    #[must_use]
    #[inline(always)]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Reports whether an existing directory is accepted.
    #[must_use]
    #[inline(always)]
    pub const fn exists_ok(&self) -> bool {
        self.exists_ok
    }

    /// Enables recursive ancestor creation.
    #[must_use]
    #[inline(always)]
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Accepts an existing directory as a successful outcome.
    #[must_use]
    #[inline(always)]
    pub const fn with_exists_ok(mut self) -> Self {
        self.exists_ok = true;
        self
    }
}
