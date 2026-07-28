// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.

use super::{
    LocalCopyMethod,
    LocalCopyStats,
};

/// Structured result of a native file or directory copy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalCopyOutcome {
    /// Aggregate copy statistics.
    stats: LocalCopyStats,
    /// Method used to perform the copy.
    method: LocalCopyMethod,
    /// Whether destination publication was atomic as one operation.
    atomic: bool,
    /// Whether required durability synchronization completed.
    durable: bool,
    /// Whether the operation crossed a device boundary.
    crossed_device: bool,
    /// Whether a lower-fidelity fallback method was used.
    fallback: bool,
}

impl LocalCopyOutcome {
    /// Creates a copy outcome from verified implementation results.
    ///
    /// # Parameters
    ///
    /// - `stats`: Aggregate copy statistics.
    /// - `method`: Native copy method.
    /// - `atomic`: Whether the whole publication was atomic.
    /// - `durable`: Whether durability synchronization completed.
    #[inline(always)]
    pub(crate) const fn new(
        stats: LocalCopyStats,
        method: LocalCopyMethod,
        atomic: bool,
        durable: bool,
    ) -> Self {
        Self {
            stats,
            method,
            atomic,
            durable,
            crossed_device: false,
            fallback: false,
        }
    }

    /// Returns aggregate copy statistics.
    #[inline(always)]
    pub const fn stats(&self) -> LocalCopyStats {
        self.stats
    }

    /// Returns the method used to copy the entry.
    #[must_use]
    #[inline(always)]
    pub const fn method(&self) -> LocalCopyMethod {
        self.method
    }

    /// Reports whether the entire destination publication was atomic.
    #[must_use]
    #[inline(always)]
    pub const fn atomic(&self) -> bool {
        self.atomic
    }

    /// Reports whether durability synchronization completed.
    #[must_use]
    #[inline(always)]
    pub const fn durable(&self) -> bool {
        self.durable
    }

    /// Reports whether a native device boundary was crossed.
    #[must_use]
    #[inline(always)]
    pub const fn crossed_device(&self) -> bool {
        self.crossed_device
    }

    /// Reports whether a lower-fidelity fallback was used.
    #[must_use]
    #[inline(always)]
    pub const fn used_fallback(&self) -> bool {
        self.fallback
    }
}
