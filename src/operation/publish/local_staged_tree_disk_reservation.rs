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
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("LocalStagedTreeDiskReservation")
            .field(&self.0.amount())
            .finish()
    }
}

impl LocalStagedTreeDiskReservation {
    /// Returns the number of disk bytes held by this reservation.
    pub fn bytes(&self) -> u64 {
        self.0.amount()
    }
}
