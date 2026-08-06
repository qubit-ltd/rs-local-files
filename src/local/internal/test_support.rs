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
use std::ffi::OsStr;
#[cfg(feature = "internal-test-support")]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(feature = "internal-test-support")]
const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";

#[cfg(feature = "internal-test-support")]
static ONE_SHOT_FAULT_TAKEN: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "internal-test-support")]
static NTH_FAULT_OCCURRENCES: AtomicUsize = AtomicUsize::new(0);

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
    std::env::var_os(TEST_FAULT_ENV).is_some_and(|value| value == OsStr::new(name))
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
