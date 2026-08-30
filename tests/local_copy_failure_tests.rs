// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Integration tests for typed unified-copy failures.

#[cfg(feature = "internal-test-support")]
use std::fs;
use std::path::PathBuf;
#[cfg(feature = "internal-test-support")]
use std::process::Command;

#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalCopyConflictPolicy;
use qubit_local_files::LocalCopyFailureState;
use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalCopyStats;
#[cfg(all(feature = "internal-test-support", not(windows)))]
use qubit_local_files::LocalDurabilityRequirement;
use qubit_local_files::LocalFileSystem;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalMetadataPreservePolicy;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::install_test_fault;

/// Creates a process-specific path that is absent before each test use.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("qubit-local-files-copy-failure-{name}-{}", std::process::id()))
}

/// Runs one test-support-only fault case in an isolated child test process.
#[cfg(feature = "internal-test-support")]
fn run_in_test_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_ENV).is_some_and(|selected| selected == std::ffi::OsStr::new(fault)) {
        let _fault = install_test_fault(fault).expect("test fault controller should install");
        action();
        return;
    }
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some() {
        return;
    }
    let executable = std::env::current_exe().expect("test executable should be available");
    let status = Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(TEST_FAULT_ENV, fault)
        .env(TEST_FAULT_CHILD_ENV, "1")
        .status()
        .expect("test fault child should launch");
    assert!(status.success(), "test fault child should pass");
}

/// Verifies preflight failures retain an unchanged typed copy state.
#[test]
fn test_copy_failure_exposes_typed_state_and_parts() {
    let source = temp_path("missing-source");
    let target = temp_path("copy-target");
    let failure = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&source, &target, &LocalCopyOptions::default())
        .expect_err("missing source must fail");

    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.partial_stats(), &LocalCopyStats::default());
    assert!(failure.staging_path().is_none());
    assert!(failure.cleanup_error().is_none());
    assert!(!target.exists());
}

/// Verifies a second-child fault retains prior recursive publication stats.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_second_child_partial_publication() {
    const TEST_NAME: &str = "test_copy_failure_reports_second_child_partial_publication";
    run_in_test_fault_process(TEST_NAME, "copy-staging-copy-second", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir(&source).expect("source directory should be created");
        fs::write(source.join("first"), b"first").expect("first child should be written");
        fs::write(source.join("second"), b"second").expect("second child should be written");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default().with_tree_source())
            .expect_err("second child staging fault must fail");

        assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
        assert_eq!(1, failure.partial_stats().files());
        assert_eq!(Some(source.as_path()), failure.request_source_path(),);
        assert_eq!(Some(target.as_path()), failure.request_target_path(),);
        let failed_source = source.join("second");
        let failed_target = target.join("second");
        assert_eq!(Some(failed_source.as_path()), failure.failed_source_path());
        assert_eq!(Some(failed_target.as_path()), failure.failed_target_path());
    });
}

/// Verifies a parent synchronization failure follows completed publication.
#[cfg(feature = "internal-test-support")]
#[cfg(not(windows))]
#[test]
fn test_copy_failure_reports_published_after_parent_sync_fault() {
    const TEST_NAME: &str = "test_copy_failure_reports_published_after_parent_sync_fault";
    run_in_test_fault_process(TEST_NAME, "copy-parent-sync", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source file should be written");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::default().with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("parent synchronization fault must fail");

        assert_eq!(LocalCopyFailureState::Published, failure.state());
        assert_eq!(1, failure.partial_stats().files());
        assert_eq!(
            b"payload",
            fs::read(&target).expect("target should remain published").as_slice()
        );
    });
}

/// Verifies staging context is retained only when cleanup also fails.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_retains_staging_only_for_cleanup_failure() {
    const TEST_NAME: &str = "test_copy_failure_retains_staging_only_for_cleanup_failure";
    run_in_test_fault_process(TEST_NAME, "copy-staging-copy-cleanup", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source file should be written");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default())
            .expect_err("staging and cleanup faults must fail");

        assert!(failure.staging_path().is_some());
        assert!(failure.cleanup_error().is_some());
    });
}

/// Verifies successful staging cleanup omits obsolete staging diagnostics.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_omits_staging_after_successful_cleanup() {
    const TEST_NAME: &str = "test_copy_failure_omits_staging_after_successful_cleanup";
    run_in_test_fault_process(TEST_NAME, "copy-staging-copy", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source file should be written");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default())
            .expect_err("staging fault must fail");

        assert!(failure.staging_path().is_none());
        assert!(failure.cleanup_error().is_none());
    });
}

/// Verifies destination preparation errors without publication are
/// conservative.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_indeterminate_for_destination_preparation_fault() {
    const TEST_NAME: &str = "test_copy_failure_reports_indeterminate_for_destination_preparation_fault";
    run_in_test_fault_process(TEST_NAME, "copy-destination-absolute", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir(&source).expect("source directory should be created");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default().with_tree_source())
            .expect_err("destination preparation fault must fail");

        assert_eq!(LocalCopyFailureState::Indeterminate, failure.state());
        assert_eq!(&LocalCopyStats::default(), failure.partial_stats());
    });
}

/// Verifies source inspection failures in the recursive pipeline prove that no
/// destination publication began.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_unchanged_for_source_inspection_fault() {
    const TEST_NAME: &str = "test_copy_failure_reports_unchanged_for_source_inspection_fault";
    run_in_test_fault_process(TEST_NAME, "copy-source-absolute", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir(&source).expect("source directory should be created");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default().with_tree_source())
            .expect_err("source inspection fault must fail before publication");

        assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
        assert_eq!(&LocalCopyStats::default(), failure.partial_stats());
        assert!(!target.exists());
    });
}

/// Verifies recursive traversal rejects a coverage-injected directory cycle.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_directory_identity_cycle() {
    const TEST_NAME: &str = "test_copy_failure_reports_directory_identity_cycle";
    run_in_test_fault_process(TEST_NAME, "copy-dir-directory-identity-cycle", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir_all(source.join("nested")).expect("nested source directory should be created");
        fs::write(source.join("nested/payload"), b"payload").expect("source payload should be written");

        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default().with_tree_source())
            .expect_err("injected directory cycle must fail");
    });
}

/// Verifies a coverage-injected staging permission failure remains typed.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_staging_permission_failure() {
    const TEST_NAME: &str = "test_copy_failure_reports_staging_permission_failure";
    run_in_test_fault_process(TEST_NAME, "copy-staging-permissions", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source payload should be written");

        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::default().with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
            )
            .expect_err("staging permission fault must fail");
    });
}

/// Verifies a test-support-only directory-statistics overflow remains typed.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_directory_statistics_overflow() {
    const TEST_NAME: &str = "test_copy_failure_reports_directory_statistics_overflow";
    run_in_test_fault_process(TEST_NAME, "copy-stats-directories", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source/nested");
        let target = directory.path().join("target");
        fs::create_dir_all(&source).expect("nested source directory should be created");
        fs::write(source.join("payload"), b"payload").expect("source payload should be written");

        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(
                &directory.path().join("source"),
                &target,
                &LocalCopyOptions::default().with_tree_source(),
            )
            .expect_err("directory statistics overflow must fail");
    });
}

/// Verifies a test-support-only skipped-file statistics overflow remains typed.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_skipped_statistics_overflow() {
    const TEST_NAME: &str = "test_copy_failure_reports_skipped_statistics_overflow";
    run_in_test_fault_process(TEST_NAME, "copy-stats-skipped", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"source").expect("source payload should be written");
        fs::write(&target, b"target").expect("target payload should be written");

        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::default().with_conflict(LocalCopyConflictPolicy::Skip),
            )
            .expect_err("skipped statistics overflow must fail");
    });
}

/// Verifies a test-support-only copied-file statistics overflow remains typed.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_file_statistics_overflow() {
    const TEST_NAME: &str = "test_copy_failure_reports_file_statistics_overflow";
    run_in_test_fault_process(TEST_NAME, "copy-stats-files", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"source").expect("source payload should be written");

        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default())
            .expect_err("file statistics overflow must fail");
    });
}

/// Verifies a test-support-only copied-byte statistics overflow remains typed.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_byte_statistics_overflow() {
    const TEST_NAME: &str = "test_copy_failure_reports_byte_statistics_overflow";
    run_in_test_fault_process(TEST_NAME, "copy-stats-bytes", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"source").expect("source payload should be written");

        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &target, &LocalCopyOptions::default())
            .expect_err("byte statistics overflow must fail");
    });
}

/// Verifies a test-support-only overwritten-entry statistics overflow remains
/// typed.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_failure_reports_overwritten_statistics_overflow() {
    const TEST_NAME: &str = "test_copy_failure_reports_overwritten_statistics_overflow";
    run_in_test_fault_process(TEST_NAME, "copy-stats-overwritten", || {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir(&source).expect("source directory should be created");
        fs::write(source.join("payload"), b"source").expect("source payload should be written");
        fs::create_dir(&target).expect("target directory should be created");
        fs::write(target.join("payload"), b"target").expect("target payload should be written");

        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::default()
                    .with_tree_source()
                    .with_conflict(LocalCopyConflictPolicy::Overwrite),
            )
            .expect_err("overwritten statistics overflow must fail");
    });
}
