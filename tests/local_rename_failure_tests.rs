// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Integration tests for typed local rename failures.

use std::path::PathBuf;

use qubit_local_files::{
    LocalFileOperation,
    LocalFileSystem,
    LocalRenameFailureState,
    LocalRenameOptions,
};

#[cfg(coverage)]
use qubit_local_files::LocalDurabilityRequirement;

/// Creates an absent process-specific path for a rename test.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "qubit-local-files-rename-failure-{name}-{}",
        std::process::id()
    ))
}

/// Runs one coverage-only rename fault case in an isolated child test process.
#[cfg(coverage)]
fn run_in_coverage_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if std::env::var_os(COVERAGE_FAULT_ENV).is_some() {
        action();
        return;
    }
    let executable =
        std::env::current_exe().expect("test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child should launch");
    assert!(status.success(), "coverage fault child should pass");
}

/// Verifies a missing source proves that the namespace remains unchanged.
#[test]
fn test_rename_missing_source_reports_unchanged() {
    let source = temp_path("missing-source");
    let target = temp_path("rename-target");
    let failure = LocalFileSystem::host()
        .rename(&source, &target, &LocalRenameOptions::default())
        .expect_err("missing source must fail");

    assert_eq!(LocalRenameFailureState::Unchanged, failure.state());
    assert_eq!(LocalFileOperation::Rename, failure.error().operation());
    let (error, state) = failure.into_parts();
    assert_eq!(LocalFileOperation::Rename, error.operation());
    assert_eq!(LocalRenameFailureState::Unchanged, state);
    assert!(!target.exists());
}

/// Verifies a parent durability fault retains the completed rename fact.
#[cfg(coverage)]
#[test]
fn test_rename_parent_durability_failure_reports_renamed() {
    const TEST_NAME: &str =
        "test_rename_parent_durability_failure_reports_renamed";
    run_in_coverage_fault_process(TEST_NAME, "rename-parent-sync", || {
        let directory =
            tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        std::fs::write(&source, b"payload").expect("source should be written");

        let failure = LocalFileSystem::host()
            .rename(
                &source,
                &target,
                &LocalRenameOptions::default()
                    .with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("parent durability fault must fail");

        assert_eq!(LocalRenameFailureState::Renamed, failure.state());
        assert!(!source.exists());
        assert!(target.exists());
    });
}

/// Verifies an I/O failure at the native boundary remains conservative.
#[cfg(coverage)]
#[test]
fn test_rename_native_io_failure_reports_indeterminate() {
    const TEST_NAME: &str =
        "test_rename_native_io_failure_reports_indeterminate";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rename-native-indeterminate",
        || {
            let directory = tempfile::tempdir()
                .expect("temporary directory should be created");
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            std::fs::write(&source, b"payload")
                .expect("source should be written");

            let failure = LocalFileSystem::host()
                .rename(&source, &target, &LocalRenameOptions::default())
                .expect_err("native I/O fault must fail");

            assert_eq!(LocalRenameFailureState::Indeterminate, failure.state());
        },
    );
}
