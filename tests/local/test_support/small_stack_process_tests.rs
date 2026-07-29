// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Isolated child-process support for stack-usage regression tests.

use std::process::Command;
use std::thread;

/// Runs `action` on a small-stack thread in an isolated child test process.
///
/// The parent process launches only the named test with `child_environment`
/// set and starts it from the stable system temporary directory. The child
/// executes `action` on a 128 KiB thread stack and returns its value on the
/// ordinary test-harness thread. A stack overflow therefore terminates only
/// the child process and is reported as an assertion failure in the parent.
///
/// # Parameters
///
/// * `test_name` - Fully qualified libtest name of the calling test.
/// * `child_environment` - Unique environment variable identifying the child.
/// * `action` - Operation whose stack usage is under test.
///
/// # Returns
///
/// `Some` with the action result in the child process, or `None` after the
/// parent process has launched and verified the child.
///
/// # Panics
///
/// Panics when the test executable cannot be located or launched, the child
/// test fails, the small-stack thread cannot be created, or `action` panics.
pub(crate) fn run_in_small_stack_process<F, T>(
    test_name: &str,
    child_environment: &str,
    action: F,
) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    if std::env::var_os(child_environment).is_none() {
        let executable = std::env::current_exe()
            .expect("current test executable should be available");
        let status = Command::new(executable)
            .arg("--exact")
            .arg(test_name)
            .arg("--nocapture")
            .env(child_environment, "1")
            .current_dir(std::env::temp_dir())
            .status()
            .expect("small-stack child test should launch");
        assert!(status.success(), "small-stack child test should pass");
        return None;
    }

    let worker = thread::Builder::new()
        .name("small-stack-filesystem-test".to_owned())
        .stack_size(128 * 1024)
        .spawn(action)
        .expect("small-stack test thread should launch");
    match worker.join() {
        Ok(value) => Some(value),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
