// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Worker-aware gates for deterministic asynchronous adapter tests.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

#[derive(Default)]
struct GateState {
    phase: Mutex<(bool, bool)>,
    changed: Condvar,
}

static GATES: OnceLock<Mutex<HashMap<String, Arc<GateState>>>> = OnceLock::new();

fn gates() -> &'static Mutex<HashMap<String, Arc<GateState>>> {
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Test-owned handle for synchronizing with one native worker boundary.
#[doc(hidden)]
pub struct TestGateGuard {
    name: String,
    state: Arc<GateState>,
}

impl TestGateGuard {
    /// Waits until a worker reaches the named gate.
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
    fn drop(&mut self) {
        self.release();
        let mut active = gates()
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

/// Installs a named gate that a native worker can hit from another thread.
///
/// Selectors are independent so async adapter tests can run concurrently with
/// different names. Dropping the guard releases a blocked worker.
#[doc(hidden)]
pub fn install_test_gate(name: &str) -> io::Result<TestGateGuard> {
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "test gate name is empty",
        ));
    }
    let state = Arc::new(GateState::default());
    let mut active = gates()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.contains_key(name) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "test gate selector is already installed",
        ));
    }
    active.insert(name.to_owned(), Arc::clone(&state));
    Ok(TestGateGuard {
        name: name.to_owned(),
        state,
    })
}

/// Pauses the current worker if the named test gate is installed.
pub(crate) fn hit_test_gate(name: &str) {
    let state = gates()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)
        .cloned();
    let Some(state) = state else { return };
    let mut phase = state
        .phase
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    phase.0 = true;
    state.changed.notify_all();
    while !phase.1 {
        phase = state
            .changed
            .wait(phase)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

#[cfg(test)]
mod tests {
    use super::{hit_test_gate, install_test_gate};

    #[test]
    fn gate_releases_a_worker_after_entered_notification() {
        let gate = install_test_gate("test-gate-worker-regression").unwrap();
        let worker = std::thread::spawn(|| hit_test_gate("test-gate-worker-regression"));
        gate.wait_until_entered();
        gate.release();
        worker.join().unwrap();
    }

    #[test]
    fn dropping_gate_releases_a_waiting_worker() {
        let gate = install_test_gate("test-gate-drop-regression").unwrap();
        let worker = std::thread::spawn(|| hit_test_gate("test-gate-drop-regression"));
        gate.wait_until_entered();
        drop(gate);
        worker.join().unwrap();
    }
}
