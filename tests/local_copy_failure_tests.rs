// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Integration tests for typed unified-copy failures.

use std::path::PathBuf;

#[cfg(coverage)]
use std::{
    fs,
    process::Command,
};

use qubit_local_files::{
    LocalCopyFailureState,
    LocalCopyOptions,
    LocalCopyStats,
    LocalFileSystem,
};

#[cfg(coverage)]
use qubit_local_files::LocalDurabilityRequirement;

/// Creates a process-specific path that is absent before each test use.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "qubit-local-files-copy-failure-{name}-{}",
        std::process::id()
    ))
}

/// Runs one coverage-only fault case in an isolated child test process.
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
    let status = Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child should launch");
    assert!(status.success(), "coverage fault child should pass");
}

/// Verifies preflight failures retain an unchanged typed copy state.
#[test]
fn test_copy_failure_exposes_typed_state_and_parts() {
    let source = temp_path("missing-source");
    let target = temp_path("copy-target");
    let failure = LocalFileSystem::host()
        .copy(&source, &target, &LocalCopyOptions::default())
        .expect_err("missing source must fail");

    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.partial_stats(), &LocalCopyStats::default());
    assert!(failure.staging_path().is_none());
    assert!(failure.cleanup_error().is_none());
    assert!(!target.exists());
}

/// Verifies a second-child fault retains prior recursive publication stats.
#[cfg(coverage)]
#[test]
fn test_copy_failure_reports_second_child_partial_publication() {
    const TEST_NAME: &str =
        "test_copy_failure_reports_second_child_partial_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "copy-staging-copy-second",
        || {
            let directory = tempfile::tempdir()
                .expect("temporary directory should be created");
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            fs::create_dir(&source)
                .expect("source directory should be created");
            fs::write(source.join("first"), b"first")
                .expect("first child should be written");
            fs::write(source.join("second"), b"second")
                .expect("second child should be written");

            let failure = LocalFileSystem::host()
                .copy(
                    &source,
                    &target,
                    &LocalCopyOptions::default().with_tree_source(),
                )
                .expect_err("second child staging fault must fail");

            assert_eq!(
                LocalCopyFailureState::PartiallyPublished,
                failure.state()
            );
            assert_eq!(1, failure.partial_stats().files());
        },
    );
}

/// Verifies a parent synchronization failure follows completed publication.
#[cfg(coverage)]
#[test]
fn test_copy_failure_reports_published_after_parent_sync_fault() {
    const TEST_NAME: &str =
        "test_copy_failure_reports_published_after_parent_sync_fault";
    run_in_coverage_fault_process(TEST_NAME, "copy-parent-sync", || {
        let directory =
            tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source file should be written");

        let failure = LocalFileSystem::host()
            .copy(
                &source,
                &target,
                &LocalCopyOptions::default()
                    .with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("parent synchronization fault must fail");

        assert_eq!(LocalCopyFailureState::Published, failure.state());
        assert_eq!(1, failure.partial_stats().files());
        assert_eq!(
            b"payload",
            fs::read(&target)
                .expect("target should remain published")
                .as_slice()
        );
    });
}

/// Verifies staging context is retained only when cleanup also fails.
#[cfg(coverage)]
#[test]
fn test_copy_failure_retains_staging_only_for_cleanup_failure() {
    const TEST_NAME: &str =
        "test_copy_failure_retains_staging_only_for_cleanup_failure";
    run_in_coverage_fault_process(
        TEST_NAME,
        "copy-staging-copy-cleanup",
        || {
            let directory = tempfile::tempdir()
                .expect("temporary directory should be created");
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            fs::write(&source, b"payload")
                .expect("source file should be written");

            let failure = LocalFileSystem::host()
                .copy(&source, &target, &LocalCopyOptions::default())
                .expect_err("staging and cleanup faults must fail");

            assert!(failure.staging_path().is_some());
            assert!(failure.cleanup_error().is_some());
        },
    );
}

/// Verifies successful staging cleanup omits obsolete staging diagnostics.
#[cfg(coverage)]
#[test]
fn test_copy_failure_omits_staging_after_successful_cleanup() {
    const TEST_NAME: &str =
        "test_copy_failure_omits_staging_after_successful_cleanup";
    run_in_coverage_fault_process(TEST_NAME, "copy-staging-copy", || {
        let directory =
            tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source file should be written");

        let failure = LocalFileSystem::host()
            .copy(&source, &target, &LocalCopyOptions::default())
            .expect_err("staging fault must fail");

        assert!(failure.staging_path().is_none());
        assert!(failure.cleanup_error().is_none());
    });
}

/// Verifies destination preparation errors without publication are
/// conservative.
#[cfg(coverage)]
#[test]
fn test_copy_failure_reports_indeterminate_for_destination_preparation_fault() {
    const TEST_NAME: &str = "test_copy_failure_reports_indeterminate_for_destination_preparation_fault";
    run_in_coverage_fault_process(
        TEST_NAME,
        "copy-destination-absolute",
        || {
            let directory = tempfile::tempdir()
                .expect("temporary directory should be created");
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            fs::create_dir(&source)
                .expect("source directory should be created");

            let failure = LocalFileSystem::host()
                .copy(
                    &source,
                    &target,
                    &LocalCopyOptions::default().with_tree_source(),
                )
                .expect_err("destination preparation fault must fail");

            assert_eq!(LocalCopyFailureState::Indeterminate, failure.state());
            assert_eq!(&LocalCopyStats::default(), failure.partial_stats());
        },
    );
}

/// Verifies source inspection failures in the recursive pipeline prove that no
/// destination publication began.
#[cfg(coverage)]
#[test]
fn test_copy_failure_reports_unchanged_for_source_inspection_fault() {
    const TEST_NAME: &str =
        "test_copy_failure_reports_unchanged_for_source_inspection_fault";
    run_in_coverage_fault_process(TEST_NAME, "copy-source-absolute", || {
        let directory =
            tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir(&source).expect("source directory should be created");

        let failure = LocalFileSystem::host()
            .copy(
                &source,
                &target,
                &LocalCopyOptions::default().with_tree_source(),
            )
            .expect_err("source inspection fault must fail before publication");

        assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
        assert_eq!(&LocalCopyStats::default(), failure.partial_stats());
        assert!(!target.exists());
    });
}
