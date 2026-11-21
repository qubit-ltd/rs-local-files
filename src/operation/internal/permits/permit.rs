// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::permit_state::PermitState;

/// Owns a portion of the capacity reserved from a permit pool.
pub(crate) struct Permit {
    /// Shared counter that enforces the pool capacity.
    pub(super) state: Arc<PermitState>,
    /// Number of units currently owned by this permit.
    pub(super) amount: usize,
}

impl Permit {
    /// Reports the number of units currently owned by this permit.
    pub(crate) fn amount(&self) -> usize {
        self.amount
    }

    /// Grows this reservation if the shared pool has enough remaining capacity.
    /// # Parameters
    /// - `amount` — additional units requested from the shared pool.
    pub(crate) fn grow(&mut self, amount: usize) -> bool {
        self.try_grow(amount).is_ok()
    }

    /// Grows the reservation and returns the pool facts observed on rejection.
    ///
    /// # Parameters
    /// - `amount` — additional units requested from the shared pool.
    ///
    /// # Returns
    /// `Ok(())` when acquired, otherwise the pool limit, available amount, and
    /// request.
    pub(crate) fn try_grow(&mut self, amount: usize) -> Result<(), (usize, usize, usize)> {
        if amount == 0 {
            return Ok(());
        }
        let Some(next_amount) = self.amount.checked_add(amount) else {
            return Err((self.state.limit, 0, amount));
        };
        let mut current = self.state.used.load(Ordering::Acquire);
        loop {
            let Some(next_used) = current.checked_add(amount) else {
                return Err((self.state.limit, self.state.limit.saturating_sub(current), amount));
            };
            if next_used > self.state.limit {
                return Err((self.state.limit, self.state.limit.saturating_sub(current), amount));
            }
            match self
                .state
                .used
                .compare_exchange_weak(current, next_used, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    self.amount = next_amount;
                    return Ok(());
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Releases up to the requested amount while keeping the remaining units.
    /// # Parameters
    /// - `amount` — maximum number of units to release.
    pub(crate) fn shrink(&mut self, amount: usize) {
        let released = amount.min(self.amount);
        self.amount -= released;
        self.state.used.fetch_sub(released, Ordering::AcqRel);
    }
}

impl Drop for Permit {
    /// Returns all units still held when the permit is dropped.
    fn drop(&mut self) {
        self.state.used.fetch_sub(self.amount, Ordering::AcqRel);
    }
}
