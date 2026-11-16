// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Guard that controls one named gate reached by a native worker.

use std::sync::Arc;

use super::gate_state::GateState;

/// Test-owned handle for synchronizing with one native worker boundary.
#[doc(hidden)]
pub struct TestGateGuard {
    /// Selector removed from the registry when this guard is dropped.
    pub(super) name: String,
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
        let mut phase = self
            .state
            .phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !phase.0 && !phase.1 {
            phase = self
                .state
                .changed
                .wait(phase)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
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
        let mut active = super::gates()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active
            .get(&self.name)
            .is_some_and(|state| Arc::ptr_eq(state, &self.state))
        {
            active.remove(&self.name);
        }
    }
}
