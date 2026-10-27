// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

#[cfg(windows)]
fn open_directory(path: &std::path::Path) -> std::fs::File {
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .expect("open directory fixture")
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_creates_protected_file_and_directory() {
    use std::os::windows::io::AsRawHandle;

    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let file = privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("private")).unwrap();
    privacy_windows::verify_current_user_access(&file, false, false).unwrap();
    assert_ne!(file.as_raw_handle(), parent.as_raw_handle());

    let directory = privacy_windows::create_private_directory_at(&parent, std::ffi::OsStr::new("private-dir")).unwrap();
    privacy_windows::verify_current_user_access(&directory, true, false).unwrap();
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_read_only_removes_data_write_access() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let file = privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("readonly")).unwrap();
    privacy_windows::apply_current_user_access(&file, false, true).unwrap();
    privacy_windows::verify_current_user_access(&file, false, true).unwrap();
}

#[cfg(windows)]
#[test]
fn test_private_windows_native_collision_kind() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("existing");
    std::fs::write(&path, b"preserve").unwrap();
    let parent = open_directory(fixture.path());
    let error = privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("existing")).unwrap_err();
    assert_eq!(error.source.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(path).unwrap(), b"preserve");
}

#[cfg(windows)]
#[test]
fn test_private_windows_native_directory_collision_kind() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    std::fs::create_dir(fixture.path().join("existing-dir")).unwrap();
    let parent = open_directory(fixture.path());
    let error =
        privacy_windows::create_private_directory_at(&parent, std::ffi::OsStr::new("existing-dir")).unwrap_err();
    assert_eq!(error.source.kind(), std::io::ErrorKind::AlreadyExists);
}

#[cfg(windows)]
#[test]
fn test_private_windows_native_missing_entry_kind() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let error = privacy_windows::open_existing_entry_at(&parent, std::ffi::OsStr::new("missing"), false).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_retains_partial_after_readback_failure() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = open_directory(fixture.path());
    privacy_windows::fail_once_at(privacy_windows::FailurePoint::AclReadback);
    let error = privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("partial")).unwrap_err();
    let retained = error.retained.expect("created entry must remain owned");
    assert_eq!(retained.name, std::ffi::OsStr::new("partial"));
    assert!(retained.file.is_some());
    assert!(fixture.path().join("partial").exists());
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_injected_precreate_failures_do_not_create_entries() {
    use crate::temp::internal::privacy_windows;

    for (point, name) in [
        (privacy_windows::FailurePoint::Token, "token"),
        (privacy_windows::FailurePoint::Sid, "sid"),
        (privacy_windows::FailurePoint::Descriptor, "descriptor"),
        (privacy_windows::FailurePoint::Create, "create"),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let parent = open_directory(fixture.path());
        privacy_windows::fail_once_at(point);
        assert!(privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new(name)).is_err());
        assert!(!fixture.path().join(name).exists());
    }
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_injected_acl_and_readonly_failures_are_reported() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = open_directory(fixture.path());
    let file = privacy_windows::create_private_file_at(&parent, std::ffi::OsStr::new("policy")).unwrap();

    privacy_windows::fail_once_at(privacy_windows::FailurePoint::AclApply);
    assert!(privacy_windows::apply_current_user_access(&file, false, false).is_err());

    privacy_windows::fail_once_at(privacy_windows::FailurePoint::ReadOnly);
    assert!(privacy_windows::apply_current_user_access(&file, false, true).is_err());
}

#[cfg(windows)]
#[test]
fn lf3_privacy_windows_nth_child_create_failure_keeps_prior_children() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().unwrap();
    let parent = open_directory(fixture.path());
    privacy_windows::fail_after(privacy_windows::FailurePoint::Create, 2);
    for name in ["child-1", "child-2"] {
        privacy_windows::create_private_directory_at(&parent, std::ffi::OsStr::new(name)).unwrap();
    }
    assert!(privacy_windows::create_private_directory_at(&parent, std::ffi::OsStr::new("child-3")).is_err());
    assert!(fixture.path().join("child-1").is_dir());
    assert!(fixture.path().join("child-2").is_dir());
    assert!(!fixture.path().join("child-3").exists());
}
