// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private fault injection support used by deterministic integration tests.

use std::io;
#[cfg(feature = "test-support")]
use std::sync::Condvar;
#[cfg(feature = "test-support")]
use std::sync::Mutex;
#[cfg(feature = "test-support")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "test-support")]
use std::sync::atomic::AtomicUsize;
#[cfg(feature = "test-support")]
use std::sync::atomic::Ordering;

#[cfg(feature = "test-support")]
use super::active_fault::ActiveFault;
#[cfg(feature = "test-support")]
use super::test_fault_guard::TestFaultGuard;

/// Whether the selected one-shot fault has already been consumed.
#[cfg(feature = "test-support")]
static ONE_SHOT_FAULT_TAKEN: AtomicBool = AtomicBool::new(false);

/// Number of times the selected occurrence-counted boundary was reached.
#[cfg(feature = "test-support")]
static NTH_FAULT_OCCURRENCES: AtomicUsize = AtomicUsize::new(0);

/// Process-local selector and waiter notification shared by integration tests.
#[cfg(feature = "test-support")]
static ACTIVE_FAULT: (Mutex<Option<ActiveFault>>, Condvar) = (Mutex::new(None), Condvar::new());

/// Installs one deterministic test fault for the current process.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub fn install_test_fault(name: &str) -> io::Result<TestFaultGuard> {
    let owner = std::thread::current().id();
    let mut active = ACTIVE_FAULT.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        match active.as_ref() {
            Some(current) if current.owner == owner => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "a test fault controller is already installed on this thread",
                ));
            }
            Some(_) => {
                active = ACTIVE_FAULT
                    .1
                    .wait(active)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            None => break,
        }
    }
    *active = Some(ActiveFault {
        owner,
        name: name.to_owned(),
    });
    ONE_SHOT_FAULT_TAKEN.store(false, Ordering::Relaxed);
    NTH_FAULT_OCCURRENCES.store(0, Ordering::Relaxed);
    Ok(TestFaultGuard { active: true })
}

#[cfg(feature = "test-support")]
impl Drop for TestFaultGuard {
    /// Releases the process-wide selector and occurrence counters.
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut active = ACTIVE_FAULT.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        active.take();
        ONE_SHOT_FAULT_TAKEN.store(false, Ordering::Relaxed);
        NTH_FAULT_OCCURRENCES.store(0, Ordering::Relaxed);
        self.active = false;
        ACTIVE_FAULT.1.notify_one();
    }
}

/// Returns a deterministic native I/O error for a selected test fault.
///
/// # Parameters
///
/// * `name` - Static selector name at a native operation boundary.
///
/// # Returns
///
/// `Some` with an I/O error when the feature is enabled and the subprocess
/// selector matches `name`; otherwise `None`.
#[must_use]
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn io_error(name: &str) -> Option<io::Error> {
    #[cfg(feature = "test-support")]
    {
        if is_enabled(name) {
            return Some(fault_error());
        }
    }
    let _ = name;
    None
}

/// Builds the platform-specific deterministic I/O failure used by selectors.
#[must_use]
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn fault_error() -> io::Error {
    #[cfg(all(feature = "test-support", unix))]
    {
        io::Error::from_raw_os_error(libc::EIO)
    }
    #[cfg(all(feature = "test-support", windows))]
    {
        io::Error::from_raw_os_error(windows_sys::Win32::Foundation::ERROR_IO_DEVICE as i32)
    }
    #[cfg(not(feature = "test-support"))]
    {
        io::Error::from(io::ErrorKind::Other)
    }
}

/// Returns whether the isolated test process selected `name`.
///
/// # Parameters
///
/// * `name` - Static selector name at a test fault boundary.
///
/// # Returns
///
/// `true` only when the feature is enabled and the subprocess selector exactly
/// matches `name`.
#[must_use]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn is_enabled(name: &str) -> bool {
    is_enabled_impl(name)
}

/// Takes the selected fault once within its isolated subprocess.
///
/// # Parameters
///
/// * `name` - Static selector name for a one-shot fault.
///
/// # Returns
///
/// `true` only for the first matching call in the subprocess.
#[cfg(feature = "test-support")]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn take(name: &str) -> bool {
    take_impl(name)
}

/// Returns whether this is the selected occurrence of one isolated fault.
///
/// # Parameters
///
/// * `name` - Static selector name for an occurrence-counted fault.
/// * `occurrence` - One-based invocation number that should fail.
///
/// # Returns
///
/// `true` only for the requested matching invocation in the subprocess.
#[cfg(feature = "test-support")]
#[must_use]
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn take_on_nth(name: &str, occurrence: usize) -> bool {
    take_on_nth_impl(name, occurrence)
}

/// Reports whether the process-local fault selector matches `name`.
///
/// # Parameters
///
/// * `name` - Fault selector to compare with the installed selector.
///
/// # Returns
/// `true` when the selector matches; otherwise `false`.
#[cfg(feature = "test-support")]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn is_enabled_impl(name: &str) -> bool {
    let owner = std::thread::current().id();
    ACTIVE_FAULT
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|active| active.owner == owner && active.name == name)
}

/// Provides the disabled-feature result for fault-selector checks.
///
/// # Returns
/// Always returns `false` when internal test support is disabled.
#[cfg(not(feature = "test-support"))]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn is_enabled_impl(_name: &str) -> bool {
    false
}

/// Takes the process-local one-shot fault when its selector matches.
///
/// # Parameters
///
/// * `name` - Fault selector to compare with the installed selector.
///
/// # Returns
/// `true` only for the first matching call after installation.
#[cfg(feature = "test-support")]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn take_impl(name: &str) -> bool {
    is_enabled_impl(name)
        && ONE_SHOT_FAULT_TAKEN
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

/// Takes a process-local fault on the requested one-based occurrence.
///
/// # Parameters
///
/// * `name` - Fault selector to compare with the installed selector.
/// * `occurrence` - One-based matching invocation number.
///
/// # Returns
/// `true` only when `name` is selected and the requested occurrence is reached.
#[cfg(feature = "test-support")]
#[must_use]
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn take_on_nth_impl(name: &str, occurrence: usize) -> bool {
    is_enabled_impl(name) && NTH_FAULT_OCCURRENCES.fetch_add(1, Ordering::Relaxed) + 1 == occurrence
}
