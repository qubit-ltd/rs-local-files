// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage-only tests for native temporary-entry creation failures.

use qubit_local_files::{
    LocalFileErrorKind,
    LocalFileSystem,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
};
use tempfile::tempdir;

use super::test_support::run_in_coverage_fault_process;

/// Verifies a generated temporary-file collision retries and retains cleanup
/// ownership of the subsequently created entry.
#[test]
fn test_temp_file_retries_injected_collision() {
    const TEST_NAME: &str = concat!(
        "local::local_temp_entry_fault_tests::",
        "test_temp_file_retries_injected_collision",
    );
    let Some(()) =
        run_in_coverage_fault_process(TEST_NAME, "temp-file-collision", || {
            let parent = tempdir().expect("temporary parent should be created");
            let mut temporary = LocalFileSystem::create_temp_file(
                &LocalTempFileOptions::new().with_parent(parent.path()),
            )
            .expect("one injected collision should be retried");
            temporary
                .cleanup()
                .expect("retried temporary file should clean up");
        })
    else {
        return;
    };
}

/// Verifies a native temporary-file creation failure is reported through the
/// public operation boundary.
#[test]
fn test_temp_file_reports_injected_creation_failure() {
    const TEST_NAME: &str = concat!(
        "local::local_temp_entry_fault_tests::",
        "test_temp_file_reports_injected_creation_failure",
    );
    let Some(()) =
        run_in_coverage_fault_process(TEST_NAME, "temp-file-open", || {
            let parent = tempdir().expect("temporary parent should be created");
            let error = LocalFileSystem::create_temp_file(
                &LocalTempFileOptions::new().with_parent(parent.path()),
            )
            .expect_err("injected file creation failure should propagate");

            assert_eq!(LocalFileErrorKind::Io, error.kind());
            assert_eq!(Some(parent.path()), error.path());
        })
    else {
        return;
    };
}

/// Verifies a generated temporary-directory collision retries and yields an
/// independently cleanup-owned directory.
#[test]
fn test_temp_directory_retries_injected_collision() {
    const TEST_NAME: &str = concat!(
        "local::local_temp_entry_fault_tests::",
        "test_temp_directory_retries_injected_collision",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "temp-directory-collision",
        || {
            let parent = tempdir().expect("temporary parent should be created");
            let mut temporary = LocalFileSystem::create_temp_directory(
                &LocalTempDirectoryOptions::new().with_parent(parent.path()),
            )
            .expect("one injected collision should be retried");
            temporary
                .cleanup()
                .expect("retried temporary directory should clean up");
        },
    ) else {
        return;
    };
}

/// Verifies a native temporary-directory creation failure is reported through
/// the public operation boundary.
#[test]
fn test_temp_directory_reports_injected_creation_failure() {
    const TEST_NAME: &str = concat!(
        "local::local_temp_entry_fault_tests::",
        "test_temp_directory_reports_injected_creation_failure",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "temp-directory-create",
        || {
            let parent = tempdir().expect("temporary parent should be created");
            let error = LocalFileSystem::create_temp_directory(
                &LocalTempDirectoryOptions::new().with_parent(parent.path()),
            )
            .expect_err("injected directory creation failure should propagate");

            assert_eq!(LocalFileErrorKind::Io, error.kind());
            assert_eq!(Some(parent.path()), error.path());
        },
    ) else {
        return;
    };
}
