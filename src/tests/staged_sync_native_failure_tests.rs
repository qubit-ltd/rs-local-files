// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(unix)]

use std::path::Path;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::policy::LocalFileSyncRequirement;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[test]
fn test_sync_observation_failures_are_never_downgraded_to_preferred_sync() {
    for requirement in [LocalFileSyncRequirement::Preferred, LocalFileSyncRequirement::Required] {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        stage
            .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
            .expect("payload");
        let path = stage.directory().diagnostic_path().join("payload");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 1);
        let error = stage
            .sync_files_with_options(&LocalListOptions::new(), requirement)
            .expect_err("an unobserved source cannot be synchronized safely");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(
            error.io_error().expect("native cause").to_string(),
            "injected native I/O contract failure"
        );
        assert_eq!(error.path(), None);
        assert_eq!(std::fs::read(&path).expect("source is unchanged"), b"payload");
        drop(guard);
        assert!(
            stage
                .sync_files_with_options(&LocalListOptions::new(), requirement)
                .expect("retry sync")
        );
        stage.cleanup().expect("cleanup after retry");
    }
}
#[cfg(target_os = "linux")]
#[test]
fn test_tree_sync_native_observation_failures_preserve_source_and_allow_retry() {
    for occurrence in [1, 2, 3] {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let policy = crate::policy::LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
            .expect("owner-policy stage");
        stage
            .write_file_new(Path::new("nested/payload"), &mut &b"payload"[..], 32)
            .expect("nested payload");
        let path = stage.directory().diagnostic_path().join("nested/payload");
        let limits = LocalListOptions::new().with_max_depth(4).with_max_seen_name_bytes(4096);
        // The file handle, nested directory handle, and final parent
        // handle are observed in this order during a complete tree sync.
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, occurrence);
        let error = stage
            .sync_tree_with_options(&parent, &limits)
            .expect_err("native observation failure must stop the synchronization phase");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(
            error.io_error().expect("native cause").to_string(),
            "injected native I/O contract failure"
        );
        assert_eq!(std::fs::read(&path).expect("source remains intact"), b"payload");
        drop(guard);
        stage
            .sync_tree_with_options(&parent, &limits)
            .expect("bounded sync retries after transient native failure");
        stage.cleanup().expect("cleanup stage after synchronization retry");
    }
}
#[cfg(target_os = "linux")]
#[test]
fn test_tree_native_sync_failures_preserve_exact_failed_path_and_allow_cleanup() {
    for occurrence in 1..=4 {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let policy = crate::policy::LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
            .expect("owner-policy stage");
        stage
            .write_file_new(Path::new("nested/payload"), &mut &b"payload"[..], 32)
            .expect("nested payload");
        let stage_path = stage.directory().diagnostic_path().to_path_buf();
        let expected_path = match occurrence {
            1 => stage_path.join("nested/payload"),
            2 => stage_path.join("nested"),
            3 => stage_path.clone(),
            _ => fixture.path().to_path_buf(),
        };
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::SyncAll, occurrence);
        let error = stage
            .sync_tree_with_options(&parent, &LocalListOptions::new())
            .expect_err("each required native sync failure must abort the current phase");
        assert!(guard.fired());
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), LocalFileOperation::Commit);
        assert_eq!(error.path(), Some(expected_path.as_path()));
        assert_eq!(
            error.io_error().expect("native cause").to_string(),
            "injected native I/O contract failure"
        );
        assert_eq!(
            std::fs::read(stage_path.join("nested/payload")).expect("preserved source"),
            b"payload"
        );
        drop(guard);
        stage
            .sync_tree_with_options(&parent, &LocalListOptions::new())
            .expect("retry failed synchronization");
        stage.cleanup().expect("cleanup after sync retry");
    }
}

#[test]
fn test_preferred_native_file_sync_failure_reports_unsynced_without_losing_owner() {
    let fixture = tempfile::tempdir().expect("fixture");
    let parent = LocalFileSystem::host()
        .expect("host")
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::SyncAll, 1);
    assert!(
        !stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::Preferred)
            .expect("preferred synchronization permits an unsynced file")
    );
    assert!(guard.fired());
    drop(guard);
    assert!(
        stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::Required)
            .expect("retry required synchronization")
    );
    stage.cleanup().expect("cleanup retryable source");
}
