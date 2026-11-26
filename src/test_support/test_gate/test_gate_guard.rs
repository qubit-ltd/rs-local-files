// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Guard that controls one named gate reached by a native worker.

use std::sync::Arc;

use super::GateKey;
use super::gate_state::GateState;

/// Test-owned handle for synchronizing with one native worker boundary.
#[doc(hidden)]
pub struct TestGateGuard {
    /// Selector removed from the registry when this guard is dropped.
    pub(super) key: GateKey,
    /// Shared signal state used by the blocked worker and test thread.
    pub(super) state: Arc<GateState>,
}

impl TestGateGuard {
    /// Waits until a worker reaches the named gate.
    ///
    /// # Synchronization
    ///
    /// Blocks the calling test thread until the worker enters the gate or the
    /// gate is released.
    pub fn wait_until_entered(&self) {
        assert!(
            self.wait_until_entered_timeout(std::time::Duration::from_secs(5)),
            "test gate {:?} was not reached within 5 seconds",
            self.key
        );
    }

    /// Waits for a worker to enter the gate up to the given duration.
    ///
    /// Returns `false` if the deadline expires before entry or release.
    pub fn wait_until_entered_timeout(&self, timeout: std::time::Duration) -> bool {
        let mut phase = self
            .state
            .phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let deadline = std::time::Instant::now() + timeout;
        while !phase.0 && !phase.1 {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, result) = self
                .state
                .changed
                .wait_timeout(phase, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            phase = next;
            if result.timed_out() && !phase.0 && !phase.1 {
                return false;
            }
        }
        phase.0
    }

    /// Releases any worker waiting at this gate.
    ///
    /// # Effects
    ///
    /// Wakes all workers waiting on this gate. The guard remains installed
    /// until it is dropped.
    pub fn release(&self) {
        let mut phase = self
            .state
            .phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        phase.1 = true;
        self.state.changed.notify_all();
    }
}

impl Drop for TestGateGuard {
    /// Releases waiters and removes this guard's selector from the registry.
    fn drop(&mut self) {
        self.release();
        let mut active = super::gates().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if active
            .get(&self.key)
            .is_some_and(|state| Arc::ptr_eq(state, &self.state))
        {
            active.remove(&self.key);
        }
    }
}
