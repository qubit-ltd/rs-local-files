// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by delete integration tests.

/// Result of deleting a native entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalDeleteOutcome {
    /// Whether an existing entry was removed.
    deleted: bool,
}

impl LocalDeleteOutcome {
    /// Creates a deletion outcome.
    ///
    /// # Parameters
    ///
    /// - `deleted`: Whether an existing entry was removed.
    #[inline]
    pub(crate) const fn new(deleted: bool) -> Self {
        Self { deleted }
    }

    /// Reports whether an entry was removed.
    #[must_use]
    #[inline(always)]
    pub const fn deleted(self) -> bool {
        self.deleted
    }
}
