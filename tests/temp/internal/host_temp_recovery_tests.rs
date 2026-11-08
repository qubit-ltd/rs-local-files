// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(all(feature = "test-support", unix))]

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

/// Verifies failed handle preparation transfers cleanup ownership for retry.
#[test]
fn test_host_temp_file_clone_failure_transfers_owned_recovery_for_retry() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let guard =
        install_test_fault("host-temp-file-recovery-clone-cleanup").expect("install file clone and cleanup failure");
    let error = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect_err("injected handle preparation failure");
    drop(guard);

    let (primary, recovery) = error.into_temp_recovery_parts();
    assert_eq!(primary.operation(), LocalFileOperation::CreateTempFile);
    let mut recovery = recovery.expect("failed preparation retains native cleanup ownership");
    assert!(recovery.identity().is_some(), "retained identity remains authoritative");
    let path = recovery.path().to_path_buf();
    assert!(path.exists(), "failed cleanup preserves the created file");
    recovery.cleanup().expect("retry cleans the file and sandbox");
    assert!(!path.exists(), "identity-checked retry removes the created file");
}

/// Verifies failed sandbox opens retain cleanup ownership for both resources.
#[test]
fn test_host_temp_sandbox_open_failure_reports_cleanup_error_and_retains_owner() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    for (operation, error) in [
        {
            let guard = install_test_fault("host-temp-sandbox-open").expect("install open failure");
            let error = filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
                .expect_err("sandbox open should fail");
            drop(guard);
            (LocalFileOperation::CreateTempFile, error)
        },
        {
            let guard = install_test_fault("host-temp-sandbox-open").expect("install open failure");
            let error = filesystem
                .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(fixture.path()))
                .expect_err("sandbox open should fail");
            drop(guard);
            (LocalFileOperation::CreateTempDirectory, error)
        },
    ] {
        assert_eq!(error.operation(), operation);
        assert_eq!(
            error.cleanup_error().expect("cleanup failure is reported").kind(),
            LocalFileErrorKind::RequirementNotMet
        );
        let recovery = error.temp_recovery().expect("sandbox recovery owner is retained");
        let path = recovery.path().to_path_buf();
        assert!(path.is_dir(), "unknown identity must preserve the residual sandbox");
        assert!(recovery.identity().is_none(), "no path-derived identity is trusted");
        let (_primary, mut recovery) = error.into_temp_recovery_parts();
        let mut recovery = recovery.take().expect("owner transfers from public error");
        assert_eq!(
            recovery
                .cleanup()
                .expect_err("unknown identity is not cleanup authority")
                .kind(),
            LocalFileErrorKind::RequirementNotMet
        );
        assert!(path.is_dir(), "cleanup must not remove by diagnostic path");
    }
}
