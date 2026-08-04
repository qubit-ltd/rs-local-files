// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Dynamic native filesystem space observations.

/// Dynamic space values observed for one filesystem authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileSystemSpace {
    capacity_bytes: Option<u64>,
    free_bytes: Option<u64>,
    available_bytes: Option<u64>,
}

impl LocalFileSystemSpace {
    /// Creates space observations from independently available values.
    #[inline(always)]
    pub const fn new(
        capacity_bytes: Option<u64>,
        free_bytes: Option<u64>,
        available_bytes: Option<u64>,
    ) -> Self {
        Self {
            capacity_bytes,
            free_bytes,
            available_bytes,
        }
    }

    /// Returns the total filesystem capacity when it can be observed.
    #[inline(always)]
    pub const fn capacity_bytes(&self) -> Option<u64> {
        self.capacity_bytes
    }

    /// Returns filesystem free capacity, including reserved space, when known.
    #[inline(always)]
    pub const fn free_bytes(&self) -> Option<u64> {
        self.free_bytes
    }

    /// Returns capacity currently available to the calling identity when known.
    #[inline(always)]
    pub const fn available_bytes(&self) -> Option<u64> {
        self.available_bytes
    }
}
