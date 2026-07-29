// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::io::Write;

use super::api_tests::LocalAtomicDestinationState;

#[cfg(all(coverage, unix))]
use super::test_support::run_in_coverage_fault_process;
use super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_atomic_commit_error_exposes_failure_and_retained_writer() {
    let dir = temp_dir("atomic-commit-error");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").expect("destination should be written");
    let mut writer = qubit_local_files::atomic::begin(&path)
        .expect("atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    fs::remove_file(&path).expect("destination should be removed");

    let mut commit_error = writer
        .commit_recoverable()
        .expect_err("pre-installation failure should retain the writer");

    assert_eq!(
        LocalAtomicDestinationState::Missing,
        commit_error.error().destination_state(),
    );
    assert!(commit_error.writer().is_some());
    assert!(commit_error.writer_mut().is_some());
    assert!(StdError::source(&commit_error).is_some());
    assert!(commit_error.to_string().contains("staging writer retained"));
    let (error, writer) = commit_error.into_parts();
    assert_eq!(
        LocalAtomicDestinationState::Missing,
        error.destination_state(),
    );
    writer
        .expect("recoverable error should return the writer")
        .abort()
        .expect("returned writer should remove staging");
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(all(coverage, unix))]
#[test]
fn test_atomic_commit_error_reports_terminal_installation_failure() {
    const TEST_NAME: &str = concat!(
        "local::local_atomic_commit_error_tests::",
        "test_atomic_commit_error_reports_terminal_installation_failure",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-install-replace",
        move || {
            let dir = temp_dir("terminal-atomic-commit-error");
            let path = dir.join("out.txt");
            fs::write(&path, b"original")
                .expect("destination should be written");
            let mut writer = qubit_local_files::atomic::begin(&path)
                .expect("atomic writer should begin");
            writer
                .write_all(b"replacement")
                .expect("replacement should be staged");

            let commit_error = writer
                .commit_recoverable()
                .expect_err("injected installation failure should be reported");

            assert!(commit_error.writer().is_none());
            assert!(
                commit_error
                    .to_string()
                    .contains("staging writer unavailable"),
            );
            fs::remove_dir_all(dir)
                .expect("terminal error fixture should be removed");
        },
    ) else {
        return;
    };
}
