// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use tempfile::tempdir;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

#[cfg(windows)]
use crate::policy::LocalOwnerAccessPolicy;
#[cfg(windows)]
use crate::temp::internal::privacy_windows;

#[cfg(windows)]
fn open_directory(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .expect("open directory fixture")
}

#[cfg(windows)]
#[test]
fn test_owner_policy_windows_create_apply_verify_uses_exact_dacl_mask() {
    let fixture = tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_0189, 0x0016_0089);
    let file = privacy_windows::create_owner_entry_at(&parent, OsStr::new("owner-policy-entry"), false, &policy)
        .expect("create exact-owner file");

    privacy_windows::verify_owner_access(&file, false, &policy).expect("verify created owner ACL");
    privacy_windows::apply_owner_access(&file, false, &policy).expect("reapply exact owner ACL");
    privacy_windows::verify_owner_access(&file, false, &policy).expect("verify applied owner ACL");
}

#[cfg(windows)]
#[test]
fn test_owner_policy_windows_directory_create_apply_verify_uses_exact_dacl_mask() {
    let fixture = tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_0189, 0x0016_0089);
    let directory =
        privacy_windows::create_owner_entry_at(&parent, OsStr::new("owner-policy-directory"), true, &policy)
            .expect("create exact-owner directory");

    privacy_windows::verify_owner_access(&directory, true, &policy).expect("verify created directory ACL");
    privacy_windows::apply_owner_access(&directory, true, &policy).expect("reapply exact directory ACL");
    privacy_windows::verify_owner_access(&directory, true, &policy).expect("verify applied directory ACL");
}

#[cfg(windows)]
#[test]
fn test_owner_policy_windows_apply_without_write_dac_fails_without_acl_expansion() {
    let fixture = tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0012_0189, 0x0012_0089);
    let file = privacy_windows::create_owner_entry_at(&parent, OsStr::new("owner-policy-read-control"), false, &policy)
        .expect("create owner file with read-control access");

    assert!(privacy_windows::apply_owner_access(&file, false, &policy).is_err());
    privacy_windows::verify_owner_access(&file, false, &policy).expect("failed apply must preserve exact ACL");
}

#[cfg(windows)]
#[test]
fn test_owner_policy_windows_missing_read_control_retains_created_resource() {
    let fixture = tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0010_0189, 0x0010_0089);

    let error =
        privacy_windows::create_owner_entry_at(&parent, OsStr::new("owner-policy-no-read-control"), false, &policy)
            .expect_err("readback without READ_CONTROL must fail");
    assert!(
        error.retained.is_some(),
        "created object must retain its recovery owner"
    );
}

#[cfg(windows)]
#[test]
fn test_owner_policy_windows_verification_failure_retains_created_resource() {
    let fixture = tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_0189, 0x0016_0089);
    privacy_windows::fail_once_at(privacy_windows::FailurePoint::AclReadback);

    let error = privacy_windows::create_owner_entry_at(&parent, OsStr::new("owner-policy-recovery"), false, &policy)
        .expect_err("injected verification failure");
    assert!(
        error.retained.is_some(),
        "created object must retain its recovery owner"
    );
}
