// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::DiskPermit;

/// A retained disk reservation associated with a staged-tree owner.
#[derive(Clone)]
pub struct LocalStagedTreeDiskReservation(
    /// Disk quota retained while the staged-tree owner is alive.
    pub(super) Arc<DiskPermit>,
);

impl std::fmt::Debug for LocalStagedTreeDiskReservation {
    /// Formats the reservation using its byte count without exposing permit internals.
    ///
    /// # Parameters
    /// * `formatter` — destination for this diagnostic representation.
    ///
    /// # Returns
    /// Formatting success or the formatter error.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("LocalStagedTreeDiskReservation")
            .field(&self.0.amount())
            .finish()
    }
}

impl LocalStagedTreeDiskReservation {
    /// Returns the number of disk bytes held by this reservation.
    ///
    /// # Returns
    /// The number of bytes represented by the retained native permit.
    ///
    /// # Parameters
    /// * `self` — reservation whose native permit is measured.
    pub fn bytes(&self) -> u64 {
        self.0.amount()
    }
}
