// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic fault selection for coverage-only subprocess tests.
// qubit-style: allow source-test-pair

use std::ffi::OsStr;
use std::sync::atomic::{
    AtomicBool,
    AtomicUsize,
    Ordering,
};

/// Environment variable carrying one isolated coverage fault name.
const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
/// Whether the selected one-shot fault was already consumed in this process.
static ONE_SHOT_FAULT_TAKEN: AtomicBool = AtomicBool::new(false);
/// Number of times the selected occurrence-counted fault has been observed.
static NTH_FAULT_OCCURRENCES: AtomicUsize = AtomicUsize::new(0);

/// Coverage-only access to selector matching.
#[cfg(coverage)]
pub fn coverage_is_enabled(name: &str) -> bool {
    is_enabled(name)
}

/// Coverage-only access to one-shot selector consumption.
#[cfg(coverage)]
pub fn coverage_take(name: &str) -> bool {
    take(name)
}

/// Coverage-only access to occurrence-counted selector consumption.
#[cfg(coverage)]
pub fn coverage_take_on_nth(name: &str, occurrence: usize) -> bool {
    take_on_nth(name, occurrence)
}

/// Returns whether the isolated test process selected `name`.
///
/// # Parameters
///
/// * `name` - Static name of a fault at a real native operation boundary.
///
/// # Returns
///
/// `true` only when the coverage subprocess selected exactly `name`.
#[must_use]
#[inline(always)]
pub(crate) fn is_enabled(name: &str) -> bool {
    std::env::var_os(COVERAGE_FAULT_ENV)
        .is_some_and(|value| value == OsStr::new(name))
}

/// Takes the selected fault once within its isolated subprocess.
///
/// # Parameters
///
/// * `name` - Static name of a one-shot fault.
///
/// # Returns
///
/// `true` only for the first matching call in the subprocess.
#[inline(always)]
pub(crate) fn take(name: &str) -> bool {
    is_enabled(name)
        && ONE_SHOT_FAULT_TAKEN
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

/// Returns whether this is the selected occurrence of one isolated fault.
///
/// # Parameters
///
/// * `name` - Static name of an occurrence-counted fault.
/// * `occurrence` - One-based matching invocation that should fail.
///
/// # Returns
///
/// `true` only for the requested matching invocation in the isolated process.
#[must_use]
#[inline]
pub(crate) fn take_on_nth(name: &str, occurrence: usize) -> bool {
    is_enabled(name)
        && NTH_FAULT_OCCURRENCES.fetch_add(1, Ordering::Relaxed) + 1
            == occurrence
}
