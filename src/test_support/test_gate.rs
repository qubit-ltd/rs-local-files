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
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use qubit_operation::ResourceId;

// Stores the shared phase and condition variable for each named gate.
mod gate_state;
// Owns the guard lifecycle and worker synchronization methods.
mod test_gate_guard;

use gate_state::GateState;
pub use test_gate_guard::TestGateGuard;

/// Selector used to locate a named gate, optionally restricted to one path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum TestGate {
    /// Gate selector scoped to a captured filesystem authority.
    Authority {
        /// Identity of the filesystem authority to match.
        identity: usize,
    },
    /// Gate selector scoped to a registered operation resource.
    Resource {
        /// Identifier of the operation resource to match.
        id: ResourceId,
    },
}
/// Key that identifies an installed gate by name, optional path, and selector.
pub(super) type GateKey = (String, Option<PathBuf>, Option<TestGate>);

/// Process-local registry of gate states.
type GateRegistry = Mutex<HashMap<GateKey, Arc<GateState>>>;

/// Registry of currently installed test gates keyed by their selector names.
static GATES: OnceLock<GateRegistry> = OnceLock::new();

/// Returns the process-wide registry used to coordinate test gates.
fn gates() -> &'static GateRegistry {
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
    install_gate(name, None, None)
}

/// Installs a named gate scoped to one captured filesystem authority.
///
/// # Parameters
///
/// * `name` is the non-empty selector used by a worker.
/// * `authority` is the filesystem instance identity to match.
#[doc(hidden)]
pub fn install_test_gate_for_authority(name: &str, authority: usize) -> io::Result<TestGateGuard> {
    install_gate(name, None, Some(TestGate::Authority { identity: authority }))
}

/// Installs a named gate scoped to one operation resource.
///
/// # Parameters
///
/// * `name` is the non-empty selector used by a worker.
/// * `resource` is the operation resource to match.
#[doc(hidden)]
pub fn install_test_gate_for_resource(name: &str, resource: ResourceId) -> io::Result<TestGateGuard> {
    install_gate(name, None, Some(TestGate::Resource { id: resource }))
}

/// Installs a named gate scoped to the authority retained by an async facade.
///
/// # Parameters
///
/// * `name` is the non-empty selector used by a worker.
/// * `filesystem` is the async facade whose authority is matched.
#[cfg(feature = "async")]
#[doc(hidden)]
pub fn install_test_gate_for_filesystem(
    name: &str,
    filesystem: &crate::operation::AsyncLocalFileSystem,
) -> io::Result<TestGateGuard> {
    let authority = Arc::as_ptr(&filesystem.filesystem.core) as usize;
    install_test_gate_for_authority(name, authority)
}

/// Installs a named gate scoped to one synchronous filesystem authority.
///
/// # Parameters
///
/// * `name` is the non-empty selector used by a worker.
/// * `filesystem` is the synchronous filesystem authority to match.
#[doc(hidden)]
pub fn install_test_gate_for_local_filesystem(
    name: &str,
    filesystem: &crate::LocalFileSystem,
) -> io::Result<TestGateGuard> {
    let authority = Arc::as_ptr(&filesystem.core) as usize;
    install_test_gate_for_authority(name, authority)
}

/// Installs a path gate scoped to one synchronous filesystem authority.
///
/// # Parameters
///
/// * `name` is the non-empty selector used by a worker.
/// * `filesystem` is the synchronous filesystem authority to match.
/// * `path` is the exact operation path allowed to enter this gate.
#[doc(hidden)]
pub fn install_test_gate_for_local_filesystem_path(
    name: &str,
    filesystem: &crate::LocalFileSystem,
    path: &Path,
) -> io::Result<TestGateGuard> {
    let authority = Arc::as_ptr(&filesystem.core) as usize;
    install_test_gate_for_authority_path(name, authority, path)
}

/// Installs a named gate that only workers operating on `path` can reach.
///
/// This keeps a test-owned gate from pausing an unrelated operation that uses
/// the same native boundary in another test running in this process.
///
/// # Parameters
///
/// - `name`: Non-empty selector used by a worker.
/// - `path`: Exact operation path allowed to enter this gate.
///
/// # Errors
///
/// Returns `InvalidInput` for an empty selector and `AlreadyExists` when the
/// same name and path are already installed.
#[doc(hidden)]
pub fn install_test_gate_for_path(name: &str, path: &Path) -> io::Result<TestGateGuard> {
    install_gate(name, Some(path.to_path_buf()), None)
}

/// Installs a path gate scoped to one filesystem authority instance.
///
/// # Parameters
///
/// * `name` is the non-empty selector used by a worker.
/// * `authority` is the filesystem instance identity to match.
/// * `path` is the exact operation path allowed to enter this gate.
#[doc(hidden)]
pub fn install_test_gate_for_authority_path(name: &str, authority: usize, path: &Path) -> io::Result<TestGateGuard> {
    install_gate(
        name,
        Some(path.to_path_buf()),
        Some(TestGate::Authority { identity: authority }),
    )
}

/// Installs a path gate scoped to the authority retained by an async facade.
///
/// # Parameters
///
/// * `name` is the non-empty selector used by a worker.
/// * `filesystem` is the async facade whose authority is matched.
/// * `path` is the exact operation path allowed to enter this gate.
#[cfg(feature = "async")]
#[doc(hidden)]
pub fn install_test_gate_for_filesystem_path(
    name: &str,
    filesystem: &crate::operation::AsyncLocalFileSystem,
    path: &Path,
) -> io::Result<TestGateGuard> {
    let authority = Arc::as_ptr(&filesystem.filesystem.core) as usize;
    install_test_gate_for_authority_path(name, authority, path)
}

/// Registers a named gate, optionally restricted to one exact operation path.
///
/// # Parameters
///
/// - `name`: Non-empty selector used by a worker.
/// - `path`: Optional exact operation path used to scope the gate.
/// - `authority`: Optional filesystem instance or operation resource selector.
///
/// # Errors
///
/// Returns `InvalidInput` for an empty selector and `AlreadyExists` when the
/// same selector is already installed.
fn install_gate(name: &str, path: Option<PathBuf>, authority: Option<TestGate>) -> io::Result<TestGateGuard> {
    if name.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "test gate name is empty"));
    }
    let state = Arc::new(GateState::default());
    let key = (name.to_owned(), path, authority);
    let mut active = gates().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.contains_key(&key) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "test gate selector is already installed",
        ));
    }
    active.insert(key.clone(), Arc::clone(&state));
    Ok(TestGateGuard { key, state })
}

/// Pauses the current worker if the named test gate is installed.
///
/// # Parameters
///
/// - `name`: Selector of the gate at which the worker should pause.
#[cfg(test)]
fn hit_test_gate(name: &str) {
    let state = gates()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(name.to_owned(), None, None))
        .cloned();
    hit_gate_state(state);
}

/// Pauses only work using the specified filesystem authority.
///
/// # Parameters
///
/// * `name` is the gate selector.
/// * `authority` is the filesystem instance identity to match.
pub(crate) fn hit_test_gate_for_authority(name: &str, authority: usize) {
    let state = gates()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(name.to_owned(), None, Some(TestGate::Authority { identity: authority })))
        .cloned();
    hit_gate_state(state);
}

/// Pauses a worker only when a path-scoped gate matches the exact operation
/// path.
///
/// # Parameters
///
/// - `name`: Selector of the gate.
/// - `path`: Operation path associated with the worker boundary.
#[cfg(test)]
pub(crate) fn hit_test_gate_for_path(name: &str, path: &Path) {
    let state = gates()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(name.to_owned(), Some(path.to_path_buf()), None))
        .cloned();
    hit_gate_state(state);
}

/// Pauses a worker only when both its path and captured authority match.
///
/// # Parameters
///
/// * `name` is the gate selector.
/// * `authority` is the filesystem instance identity to match.
/// * `path` is the operation path to match.
pub(crate) fn hit_test_gate_for_authority_path(name: &str, authority: usize, path: &Path) {
    let state = gates()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(
            name.to_owned(),
            Some(path.to_path_buf()),
            Some(TestGate::Authority { identity: authority }),
        ))
        .cloned();
    hit_gate_state(state);
}

/// Pauses only work using the specified registered resource.
///
/// # Parameters
///
/// * `name` is the gate selector.
/// * `resource` is the operation resource to match.
pub(crate) fn hit_test_gate_for_resource(name: &str, resource: ResourceId) {
    let state = gates()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(name.to_owned(), None, Some(TestGate::Resource { id: resource })))
        .cloned();
    hit_gate_state(state);
}

/// Marks an installed gate as entered and blocks until its guard releases it.
///
/// # Parameters
///
/// - `state`: Gate state to signal and wait on, or `None` when no gate matched.
fn hit_gate_state(state: Option<Arc<GateState>>) {
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
    use super::hit_test_gate_for_authority_path;
    use super::hit_test_gate_for_path;
    use super::install_test_gate;
    use super::install_test_gate_for_authority_path;
    use super::install_test_gate_for_path;

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

    #[test]
    fn test_path_scoped_gate_ignores_workers_for_other_paths() {
        use std::path::Path;
        use std::sync::mpsc;
        use std::time::Duration;

        let gate = install_test_gate_for_path("path-scoped-test-gate", Path::new("/target/one")).unwrap();
        let (unrelated_tx, unrelated_rx) = mpsc::channel();
        let unrelated = std::thread::spawn(move || {
            hit_test_gate_for_path("path-scoped-test-gate", Path::new("/target/two"));
            unrelated_tx.send(()).unwrap();
        });
        unrelated_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("a different target does not enter the installed gate");
        unrelated.join().unwrap();

        let (matching_tx, matching_rx) = mpsc::channel();
        let matching = std::thread::spawn(move || {
            hit_test_gate_for_path("path-scoped-test-gate", Path::new("/target/one"));
            matching_tx.send(()).unwrap();
        });
        gate.wait_until_entered();
        assert!(matching_rx.try_recv().is_err(), "the matching worker waits for release");
        gate.release();
        matching_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the matching target is released");
        matching.join().unwrap();
    }

    #[test]
    fn test_gate_wait_reports_when_target_was_not_reached() {
        let gate = install_test_gate("unreached-gate-diagnostic").unwrap();

        assert!(!gate.wait_until_entered_timeout(std::time::Duration::from_millis(1)));
    }

    #[test]
    fn test_authority_scoped_gate_ignores_same_path_from_other_authority() {
        use std::path::Path;
        use std::sync::mpsc;
        use std::time::Duration;

        let gate = install_test_gate_for_authority_path("authority-path-gate", 1, Path::new("same/path")).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            hit_test_gate_for_authority_path("authority-path-gate", 2, Path::new("same/path"));
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(2))
            .expect("different authority proceeds");
        assert!(!gate.wait_until_entered_timeout(Duration::from_millis(1)));
    }
}
