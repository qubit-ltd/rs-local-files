// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by rename integration tests.

use crate::policy::LocalDurabilityRequirement;

/// Options for renaming a native filesystem entry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "rename options have no effect unless they are used"]
pub struct LocalRenameOptions {
    /// Whether an existing destination may be replaced.
    overwrite: bool,
    /// Required durability.
    durability: LocalDurabilityRequirement,
}

impl LocalRenameOptions {
    /// Creates no-replace rename options without required durability.
    pub const fn new() -> Self {
        Self {
            overwrite: false,
            durability: LocalDurabilityRequirement::NotRequired,
        }
    }

    /// Reports whether an existing destination may be replaced.
    #[must_use]
    pub const fn overwrite(&self) -> bool {
        self.overwrite
    }

    /// Returns the requested durability.
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Allows replacement of an existing destination entry.
    pub const fn with_overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }

    /// Sets the required durability.
    pub const fn with_durability(
        mut self,
        requirement: LocalDurabilityRequirement,
    ) -> Self {
        self.durability = requirement;
        self
    }
}
