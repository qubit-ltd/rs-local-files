// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::LocalFileSystem;
use crate::directory::LocalTreeEntry;
use crate::directory::LocalTreeValidationOptions;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::outcome::LocalFileKind;
use crate::policy::LocalPrivacyRequirement;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[test]
fn test_private_tree_validation_rejects_root_and_child_access_without_mutation() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let native_parent = fs::canonicalize(fixture.path()).expect("parent should resolve");
    let parent = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(&native_parent, &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should exist");
    stage
        .write_file_new(Path::new("payload"), &mut &b"protected"[..], 9)
        .expect("tracked payload should exist");
    let native_stage = native_parent.join(stage.relative_name());
    let native_file = native_stage.join("payload");
    fs::set_permissions(&native_stage, fs::Permissions::from_mode(0o755)).expect("root should have public access");
    fs::set_permissions(&native_file, fs::Permissions::from_mode(0o644)).expect("child should have public access");
    let entries = [
        LocalTreeEntry::new(Path::new("payload").to_path_buf(), LocalFileKind::File).expect("entry should validate"),
    ];
    stage
        .verify_entries(&entries, &LocalTreeValidationOptions::default())
        .expect("default validation accepts public access");
    let required = LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required);
    let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 2);
    let error = stage
        .verify_entries(&entries, &required)
        .expect_err("privacy root inspection must retain native errors");
    assert!(fault.fired());
    assert_eq!(error.kind(), LocalFileErrorKind::Io);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(native_stage.as_path()));
    drop(fault);
    let error = stage
        .verify_entries(&entries, &required)
        .expect_err("public root must fail required privacy");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(error.path(), Some(native_stage.as_path()));
    assert_eq!(
        fs::metadata(&native_stage)
            .expect("root should remain")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    fs::set_permissions(&native_stage, fs::Permissions::from_mode(0o700)).expect("root privacy should be repaired");
    let error = stage
        .verify_entries(&entries, &required)
        .expect_err("public child must also fail required privacy");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(native_file.as_path()));
    assert_eq!(
        fs::metadata(&native_file)
            .expect("child should remain")
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    assert_eq!(
        fs::read(&native_file).expect("validation must preserve bytes"),
        b"protected"
    );
    fs::set_permissions(&native_file, fs::Permissions::from_mode(0o600)).expect("child privacy should be repaired");
    stage
        .verify_entries(&entries, &required)
        .expect("repaired tree should validate");
    stage.cleanup().expect("same retained owner should clean up");
    assert!(!native_stage.exists());
}

#[test]
fn test_cleanup_name_storage_budget_preserves_the_owner_for_retry() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let parent_path = fs::canonicalize(fixture.path()).expect("parent should resolve");
    fs::write(parent_path.join("sentinel"), b"keep").expect("unrelated sentinel should exist");
    let parent = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(&parent_path, &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should exist");
    stage
        .write_file_new(Path::new("payload"), &mut &b"protected"[..], 9)
        .expect("tracked payload should exist");
    let native_stage = parent_path.join(stage.relative_name());
    let identity = stage.identity().cloned();
    stage.set_cleanup_limits(
        crate::options::LocalTempCleanupLimits::new()
            .with_max_pending_path_bytes(native_stage.as_os_str().as_encoded_bytes().len()),
    );
    let failure = stage
        .cleanup()
        .expect_err("root path and child name must share the cleanup storage budget");
    assert_eq!(failure.cause().kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(failure.cause().operation(), LocalFileOperation::Cleanup);
    assert_eq!(
        fs::read(native_stage.join("payload")).expect("budget failure must not delete the child"),
        b"protected"
    );
    let (_, mut owners) = failure.into_parts();
    assert_eq!(owners.len(), 1);
    let mut owner = owners.pop().expect("failed cleanup must transfer its owner");
    assert_eq!(owner.identity(), identity.as_ref());
    owner.set_cleanup_limits(crate::options::LocalTempCleanupLimits::new());
    owner
        .cleanup()
        .expect("same owner should retry after the storage budget is raised");
    assert!(!native_stage.exists());
    assert_eq!(
        fs::read(parent_path.join("sentinel")).expect("cleanup should preserve unrelated entries"),
        b"keep"
    );
}
