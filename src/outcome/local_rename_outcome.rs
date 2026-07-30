// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by rename integration tests.

/// Guarantees actually achieved by a native rename.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalRenameOutcome {
    /// Whether namespace replacement was atomic.
    atomic: bool,
    /// Whether the destination parent was synchronized.
    durable: bool,
}

impl LocalRenameOutcome {
    /// Creates a rename outcome.
    ///
    /// # Parameters
    ///
    /// - `atomic`: Whether the namespace change was atomic.
    /// - `durable`: Whether parent-directory durability was synchronized.
    #[inline]
    pub(crate) const fn new(atomic: bool, durable: bool) -> Self {
        Self { atomic, durable }
    }

    /// Reports whether the namespace change was atomic.
    #[must_use]
    #[inline(always)]
    pub const fn atomic(self) -> bool {
        self.atomic
    }

    /// Reports whether parent-directory durability was synchronized.
    #[must_use]
    #[inline(always)]
    pub const fn durable(self) -> bool {
        self.durable
    }
}
