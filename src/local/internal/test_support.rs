// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private fault injection support used by deterministic integration tests.

use std::io;
#[cfg(feature = "internal-test-support")]
use std::sync::Mutex;
#[cfg(feature = "internal-test-support")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "internal-test-support")]
use std::sync::atomic::AtomicUsize;
#[cfg(feature = "internal-test-support")]
use std::sync::atomic::Ordering;

#[cfg(feature = "internal-test-support")]
use super::test_fault_guard::TestFaultGuard;

#[cfg(feature = "internal-test-support")]
static ONE_SHOT_FAULT_TAKEN: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "internal-test-support")]
static NTH_FAULT_OCCURRENCES: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "internal-test-support")]
static ACTIVE_FAULT: Mutex<Option<String>> = Mutex::new(None);

/// Installs one deterministic test fault for the current process.
#[cfg(feature = "internal-test-support")]
#[doc(hidden)]
pub fn install_test_fault(name: &str) -> io::Result<TestFaultGuard> {
    let mut active = ACTIVE_FAULT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a test fault controller is already installed",
        ));
    }
    *active = Some(name.to_owned());
    ONE_SHOT_FAULT_TAKEN.store(false, Ordering::Relaxed);
    NTH_FAULT_OCCURRENCES.store(0, Ordering::Relaxed);
    Ok(TestFaultGuard { active: true })
}

#[cfg(feature = "internal-test-support")]
impl Drop for TestFaultGuard {
    /// Releases the process-wide selector and occurrence counters.
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut active = ACTIVE_FAULT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        active.take();
        ONE_SHOT_FAULT_TAKEN.store(false, Ordering::Relaxed);
        NTH_FAULT_OCCURRENCES.store(0, Ordering::Relaxed);
        self.active = false;
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
#[inline(always)]
pub(crate) fn io_error(name: &str) -> Option<io::Error> {
    #[cfg(feature = "internal-test-support")]
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
#[inline]
pub(crate) fn fault_error() -> io::Error {
    #[cfg(all(feature = "internal-test-support", unix))]
    {
        io::Error::from_raw_os_error(libc::EIO)
    }
    #[cfg(all(feature = "internal-test-support", windows))]
    {
        io::Error::from_raw_os_error(windows_sys::Win32::Foundation::ERROR_IO_DEVICE as i32)
    }
    #[cfg(not(feature = "internal-test-support"))]
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
#[inline(always)]
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
#[cfg(feature = "internal-test-support")]
#[inline(always)]
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
#[cfg(feature = "internal-test-support")]
#[must_use]
#[inline]
pub(crate) fn take_on_nth(name: &str, occurrence: usize) -> bool {
    take_on_nth_impl(name, occurrence)
}

#[cfg(feature = "internal-test-support")]
#[inline(always)]
fn is_enabled_impl(name: &str) -> bool {
    ACTIVE_FAULT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_deref()
        == Some(name)
}

#[cfg(not(feature = "internal-test-support"))]
#[inline(always)]
fn is_enabled_impl(_name: &str) -> bool {
    false
}

#[cfg(feature = "internal-test-support")]
#[inline(always)]
fn take_impl(name: &str) -> bool {
    is_enabled_impl(name)
        && ONE_SHOT_FAULT_TAKEN
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

#[cfg(feature = "internal-test-support")]
#[must_use]
#[inline]
fn take_on_nth_impl(name: &str, occurrence: usize) -> bool {
    is_enabled_impl(name) && NTH_FAULT_OCCURRENCES.fetch_add(1, Ordering::Relaxed) + 1 == occurrence
}
