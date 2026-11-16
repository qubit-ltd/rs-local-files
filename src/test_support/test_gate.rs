// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Worker-aware gates for deterministic asynchronous adapter tests.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::OnceLock;

mod gate_state;

use gate_state::GateState;

/// Registry of currently installed test gates keyed by their selector names.
static GATES: OnceLock<Mutex<HashMap<String, Arc<GateState>>>> = OnceLock::new();

/// Returns the process-wide registry used to coordinate test gates.
fn gates() -> &'static Mutex<HashMap<String, Arc<GateState>>> {
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Test-owned handle for synchronizing with one native worker boundary.
#[doc(hidden)]
pub struct TestGateGuard {
    /// Selector removed from the registry when the guard is dropped.
    name: String,
    /// Shared signal state used by the blocked worker and test thread.
    state: Arc<GateState>,
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
///
/// # Parameters
///
/// - `name`: Non-empty selector used by a worker to find this gate.
///
/// # Errors
///
/// Returns `InvalidInput` for an empty selector and `AlreadyExists` when the
/// same selector is already installed.
///
/// # Returns
///
/// Returns a guard that releases blocked workers and removes the selector when
/// dropped.
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
///
/// # Parameters
///
/// - `name`: Selector of the gate at which the worker should pause.
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
    use super::hit_test_gate;
    use super::install_test_gate;

    #[test]
    fn test_gate_releases_worker_after_entered_notification() {
        let gate = install_test_gate("test-gate-worker-regression").unwrap();
        let worker = std::thread::spawn(|| hit_test_gate("test-gate-worker-regression"));
        gate.wait_until_entered();
        gate.release();
        worker.join().unwrap();
    }

    #[test]
    fn test_dropping_gate_releases_waiting_worker() {
        let gate = install_test_gate("test-gate-drop-regression").unwrap();
        let worker = std::thread::spawn(|| hit_test_gate("test-gate-drop-regression"));
        gate.wait_until_entered();
        drop(gate);
        worker.join().unwrap();
    }
}
