// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use crate::operation::internal::permits::DiskPermit;

/// A retained disk reservation associated with a staged-tree owner.
#[derive(Clone)]
pub struct LocalStagedTreeDiskReservation(
    /// Disk quota retained while the staged-tree owner is alive.
    pub(in crate::operation) Arc<DiskPermit>,
);

impl std::fmt::Debug for LocalStagedTreeDiskReservation {
    /// Formats the reservation using its byte count without exposing permit
    /// internals.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::LocalStagedTreeDiskReservation;
    use crate::operation::internal::permits::DiskPermitPool;

    #[test]
    fn test_bytes_and_debug_report_the_retained_quota() {
        let pool = DiskPermitPool::new(16);
        let reservation = LocalStagedTreeDiskReservation(Arc::new(pool.try_acquire(9).unwrap()));

        assert_eq!(reservation.bytes(), 9);
        assert_eq!(format!("{reservation:?}"), "LocalStagedTreeDiskReservation(9)");
        assert_eq!(pool.used(), 9);
        drop(reservation);
        assert_eq!(pool.used(), 0);
    }
}
