// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::permit::Permit;
use super::permit_state::PermitState;

/// Atomic bounded counter whose permits return exactly their reserved amount.
pub(crate) struct PermitPool {
    /// Counter shared by every permit from this pool.
    state: Arc<PermitState>,
}

impl PermitPool {
    /// Creates a counter with the given maximum number of units.
    /// # Parameters
    /// - `limit` — maximum number of units the pool may reserve.
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            state: Arc::new(PermitState {
                used: std::sync::atomic::AtomicUsize::new(0),
                limit,
            }),
        }
    }

    /// Reserves the requested units when the configured bound permits it.
    /// # Parameters
    /// - `amount` — number of units to reserve.
    pub(crate) fn try_acquire(&self, amount: usize) -> Option<Permit> {
        let mut permit = Permit {
            state: Arc::clone(&self.state),
            amount: 0,
        };
        permit.grow(amount).then_some(permit)
    }

    /// Returns the number of units currently held by the pool.
    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.state.used.load(Ordering::Acquire)
    }
}
