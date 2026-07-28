// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by public capability integration tests.

use super::LocalPathLengthUnit;

/// Known native path-length limit and its platform-specific unit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LocalPathLimit {
    /// Maximum known path length.
    value: usize,
    /// Unit used to measure the maximum.
    unit: LocalPathLengthUnit,
}

impl LocalPathLimit {
    /// Creates a native path-length limit.
    ///
    /// # Parameters
    ///
    /// - `value`: Maximum supported length.
    /// - `unit`: Unit used by the platform API.
    #[must_use]
    #[inline(always)]
    pub const fn new(value: usize, unit: LocalPathLengthUnit) -> Self {
        Self { value, unit }
    }

    /// Returns the maximum known path length.
    #[must_use]
    #[inline(always)]
    pub const fn value(self) -> usize {
        self.value
    }

    /// Returns the unit used to measure the limit.
    #[must_use]
    #[inline(always)]
    pub const fn unit(self) -> LocalPathLengthUnit {
        self.unit
    }
}
