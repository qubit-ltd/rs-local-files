// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::disk_permit::DiskPermit;
use super::disk_permit_state::DiskPermitState;

/// Atomic byte counter that bounds temporary disk reservations.
#[allow(dead_code)]
pub(crate) struct DiskPermitPool {
    /// Counter shared by every disk permit from this pool.
    state: Arc<DiskPermitState>,
}

#[allow(dead_code)]
impl DiskPermitPool {
    /// Creates a pool with the given temporary byte capacity.
    /// # Parameters
    /// - `limit` — maximum number of temporary bytes the pool may reserve.
    pub(crate) fn new(limit: u64) -> Self {
        Self {
            state: Arc::new(DiskPermitState {
                used: std::sync::atomic::AtomicU64::new(0),
                limit,
            }),
        }
    }

    /// Reserves the requested bytes when capacity is available.
    /// # Parameters
    /// - `amount` — number of temporary bytes to reserve.
    pub(crate) fn try_acquire(&self, amount: u64) -> Option<DiskPermit> {
        let mut permit = DiskPermit {
            state: Arc::clone(&self.state),
            amount: 0,
        };
        permit.grow(amount).then_some(permit)
    }

    /// Returns the number of temporary bytes currently reserved.
    pub(crate) fn used(&self) -> u64 {
        self.state.used.load(Ordering::Acquire)
    }
}
