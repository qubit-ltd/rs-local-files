// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::disk_permit_state::DiskPermitState;

/// Owns temporary disk capacity reserved from a disk permit pool.
#[allow(dead_code)]
pub(crate) struct DiskPermit {
    /// Shared counter that enforces the disk byte capacity.
    pub(super) state: Arc<DiskPermitState>,
    /// Number of temporary bytes currently owned by this permit.
    pub(super) amount: u64,
}

#[allow(dead_code)]
impl DiskPermit {
    /// Reports the temporary byte count currently held by this permit.
    pub(crate) fn amount(&self) -> u64 {
        self.amount
    }

    /// Grows this reservation if the shared pool has sufficient capacity.
    /// # Parameters
    /// - `amount` — additional temporary bytes requested.
    pub(crate) fn grow(&mut self, amount: u64) -> bool {
        let Some(next_amount) = self.amount.checked_add(amount) else {
            return false;
        };
        let mut current = self.state.used.load(Ordering::Acquire);
        loop {
            let Some(next_used) = current.checked_add(amount) else {
                return false;
            };
            if next_used > self.state.limit {
                return false;
            }
            match self
                .state
                .used
                .compare_exchange_weak(current, next_used, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    self.amount = next_amount;
                    return true;
                }
                Err(actual) => current = actual,
            }
        }
    }
}

#[allow(dead_code)]
impl Drop for DiskPermit {
    /// Returns all temporary bytes still held when this permit is dropped.
    fn drop(&mut self) {
        self.state.used.fetch_sub(self.amount, Ordering::AcqRel);
    }
}
