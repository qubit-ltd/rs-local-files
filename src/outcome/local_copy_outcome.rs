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
use crate::LocalMetadataPreservePolicy;

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
    /// Metadata preservation actually applied by the copy pipeline.
    metadata_preservation: LocalMetadataPreservePolicy,
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
    /// - `metadata_preservation`: Metadata preservation applied to copied
    ///   entries.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub(crate) const fn new(
        stats: LocalCopyStats,
        method: LocalCopyMethod,
        atomic: bool,
        durable: bool,
        metadata_preservation: LocalMetadataPreservePolicy,
    ) -> Self {
        Self {
            stats,
            method,
            atomic,
            durable,
            metadata_preservation,
        }
    }

    /// Returns aggregate copy statistics.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn stats(&self) -> LocalCopyStats {
        self.stats
    }

    /// Returns the method used to copy the entry.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn method(&self) -> LocalCopyMethod {
        self.method
    }

    /// Reports whether the entire destination publication was atomic.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn atomic(&self) -> bool {
        self.atomic
    }

    /// Reports whether durability synchronization completed.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn durable(&self) -> bool {
        self.durable
    }

    /// Returns metadata preservation applied by the copy pipeline.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn metadata_preservation(&self) -> LocalMetadataPreservePolicy {
        self.metadata_preservation
    }
}
