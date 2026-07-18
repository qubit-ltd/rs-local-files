// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic fault selection for coverage-only subprocess tests.

use std::ffi::OsStr;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

/// Environment variable carrying one isolated coverage fault name.
const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
/// Whether the selected one-shot fault was already consumed in this process.
static ONE_SHOT_FAULT_TAKEN: AtomicBool = AtomicBool::new(false);

/// Returns whether the isolated test process selected `name`.
///
/// # Parameters
///
/// * `name` - Static name of a fault at a real native operation boundary.
///
/// # Returns
///
/// `true` only when the coverage subprocess selected exactly `name`.
pub(super) fn is_enabled(name: &str) -> bool {
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
pub(super) fn take(name: &str) -> bool {
    is_enabled(name)
        && ONE_SHOT_FAULT_TAKEN
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}
