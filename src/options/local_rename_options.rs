// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by rename integration tests.

use super::{LocalAtomicityRequirement, LocalDurabilityRequirement};

/// Options for renaming a native filesystem entry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "rename options have no effect unless they are used"]
pub struct LocalRenameOptions {
    /// Whether an existing destination may be replaced.
    overwrite: bool,
    /// Required atomicity.
    atomicity: LocalAtomicityRequirement,
    /// Required durability.
    durability: LocalDurabilityRequirement,
}

impl LocalRenameOptions {
    /// Creates no-replace rename options with preferred atomicity.
    #[inline]
    pub const fn new() -> Self {
        Self {
            overwrite: false,
            atomicity: LocalAtomicityRequirement::Preferred,
            durability: LocalDurabilityRequirement::NotRequired,
        }
    }

    /// Reports whether an existing destination may be replaced.
    #[must_use]
    #[inline(always)]
    pub const fn overwrite(&self) -> bool {
        self.overwrite
    }

    /// Returns the requested atomicity.
    #[inline(always)]
    pub const fn atomicity(&self) -> LocalAtomicityRequirement {
        self.atomicity
    }

    /// Returns the requested durability.
    #[inline(always)]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Allows replacement of an existing destination entry.
    #[inline(always)]
    pub const fn with_overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }

    /// Sets the required atomicity.
    #[inline(always)]
    pub const fn with_atomicity(mut self, requirement: LocalAtomicityRequirement) -> Self {
        self.atomicity = requirement;
        self
    }

    /// Sets the required durability.
    #[inline(always)]
    pub const fn with_durability(mut self, requirement: LocalDurabilityRequirement) -> Self {
        self.durability = requirement;
        self
    }
}
