// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by directory integration tests.

/// Result of creating a native directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalCreateDirectoryOutcome {
    /// Whether the requested directory was absent before the operation.
    created: bool,
}

impl LocalCreateDirectoryOutcome {
    /// Creates a directory-creation outcome.
    ///
    /// # Parameters
    ///
    /// - `created`: Whether the requested entry was newly created.
    pub(crate) const fn new(created: bool) -> Self {
        Self { created }
    }

    /// Reports whether the requested directory was newly created.
    #[must_use]
    #[inline(always)]
    pub const fn created(self) -> bool {
        self.created
    }
}
