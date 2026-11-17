// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Worker-aware gates for deterministic asynchronous adapter tests.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

// Stores the shared phase and condition variable for each named gate.
mod gate_state;
// Owns the guard lifecycle and worker synchronization methods.
mod test_gate_guard;

use gate_state::GateState;
pub use test_gate_guard::TestGateGuard;

/// Registry of currently installed test gates keyed by their selector names.
static GATES: OnceLock<Mutex<HashMap<String, Arc<GateState>>>> = OnceLock::new();

/// Returns the process-wide registry used to coordinate test gates.
fn gates() -> &'static Mutex<HashMap<String, Arc<GateState>>> {
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
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
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "test gate name is empty"));
    }
    let state = Arc::new(GateState::default());
    let mut active = gates().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let mut phase = state.phase.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
