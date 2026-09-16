// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Integration tests for typed local rename failures.

use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalRenameOptions;
use qubit_local_files::outcome::LocalRenameFailureState;
#[cfg(all(feature = "test-support", not(windows)))]
use qubit_local_files::policy::LocalDurabilityRequirement;
#[cfg(feature = "test-support")]
use qubit_local_files::test_support::install_test_fault;

/// Creates an absent process-specific path for a rename test.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "qubit-local-files-rename-failure-{name}-{}",
        std::process::id()
    ))
}

/// Runs one test-support-only rename fault case in an isolated child test
/// process.
#[cfg(feature = "test-support")]
fn run_in_test_fault_process<F>(_test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    let _fault = install_test_fault(fault).expect("test fault controller should install");
    action();
}

/// Verifies a missing source proves that the namespace remains unchanged.
#[test]
fn test_rename_missing_source_reports_unchanged() {
    let source = temp_path("missing-source");
    let target = temp_path("rename-target");
    let failure = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(&source, &target, &LocalRenameOptions::default())
        .expect_err("missing source must fail");

    assert_eq!(LocalRenameFailureState::Unchanged, failure.state());
    assert_eq!(LocalFileOperation::Rename, failure.error().operation());
    let (error, state) = failure.into_parts();
    assert_eq!(LocalFileOperation::Rename, error.operation());
    assert_eq!(LocalRenameFailureState::Unchanged, state);
    assert!(!target.exists());
}

/// Verifies a parent durability fault retains the completed rename fact.
#[cfg(all(feature = "test-support", not(windows)))]
#[test]
fn test_rename_parent_durability_failure_reports_renamed() {
    const TEST_NAME: &str = "test_rename_parent_durability_failure_reports_renamed";
    run_in_test_fault_process(TEST_NAME, "rename-parent-sync", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        std::fs::write(&source, b"payload").expect("source should be written");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .rename_with_options(
                &source,
                &target,
                &LocalRenameOptions::default().with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("parent durability fault must fail");

        assert_eq!(LocalRenameFailureState::Renamed, failure.state());
        assert!(!source.exists());
        assert!(target.exists());
    });
}

/// Verifies an I/O failure at the native boundary remains conservative.
#[cfg(feature = "test-support")]
#[test]
fn test_rename_native_io_failure_reports_indeterminate() {
    const TEST_NAME: &str = "test_rename_native_io_failure_reports_indeterminate";
    run_in_test_fault_process(TEST_NAME, "rename-native-indeterminate", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        std::fs::write(&source, b"payload").expect("source should be written");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .rename_with_options(&source, &target, &LocalRenameOptions::default())
            .expect_err("native I/O fault must fail");

        assert_eq!(LocalRenameFailureState::Indeterminate, failure.state());
    });
}
