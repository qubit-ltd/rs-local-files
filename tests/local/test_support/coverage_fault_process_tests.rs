// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Isolated subprocess support for coverage-only native fault tests.

use std::process::Command;

/// Environment variable consumed by the coverage-only library fault selector.
const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";

/// Runs an action with one native fault selected in an isolated child test.
///
/// The parent launches only `test_name`, verifies that it passes, and returns
/// `None`. The child executes `action` and returns its result as `Some(T)`.
/// Process isolation prevents the fault environment from affecting parallel
/// tests in the parent process.
///
/// # Parameters
///
/// * `test_name` - Fully qualified libtest name of the calling test.
/// * `fault` - Static native fault name selected for the child.
/// * `action` - Public operation and assertions executed in the child.
///
/// # Returns
///
/// `Some` with the action result in the child, or `None` after the parent has
/// launched and verified the child.
///
/// # Panics
///
/// Panics when the current test executable is unavailable, the child cannot be
/// launched, or the child test fails.
pub(crate) fn run_in_coverage_fault_process<F, T>(
    test_name: &str,
    fault: &str,
    action: F,
) -> Option<T>
where
    F: FnOnce() -> T,
{
    if std::env::var_os(COVERAGE_FAULT_ENV).is_some() {
        return Some(action());
    }

    let executable = std::env::current_exe()
        .expect("current test executable should be available");
    let status = Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child test should launch");
    assert!(status.success(), "coverage fault child test should pass");
    None
}
