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
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ARCHIVE;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;

#[cfg(windows)]
use crate::temp::internal::privacy_windows::apply_current_user_access;
#[cfg(windows)]
use crate::temp::internal::privacy_windows::create_private_directory_at;
#[cfg(windows)]
use crate::temp::internal::privacy_windows::create_private_file_at;
#[cfg(windows)]
use crate::temp::internal::privacy_windows::verify_current_user_access;

#[cfg(windows)]
fn basic_info(file: &File) -> FILE_BASIC_INFO {
    let mut information = unsafe { std::mem::zeroed::<FILE_BASIC_INFO>() };
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&mut information as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "query retained-handle FILE_BASIC_INFO",
    );
    information
}

#[cfg(windows)]
fn test_parent() -> (tempfile::TempDir, File) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(directory.path())
        .expect("open temporary directory handle");
    (directory, parent)
}

#[cfg(windows)]
#[test]
fn test_review_windows_access_transition_preserves_readonly_attribute_and_acl() {
    let (directory, parent) = test_parent();
    let file =
        create_private_file_at(&parent, OsStr::new("access-transition-file")).expect("create current-user-only file");
    let path = directory.path().join("access-transition-file");

    verify_current_user_access(&file, false, false).expect("initial current-user-only DACL");
    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("initial file permits a write handle");

    let before = basic_info(&file);
    let retained = FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_ARCHIVE;
    let mut marked = before;
    marked.FileAttributes |= retained;
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&marked as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "set HIDDEN and ARCHIVE attributes",
    );
    let before = basic_info(&file);
    assert_eq!(before.FileAttributes & retained, retained);
    apply_current_user_access(&file, false, true).expect("apply read-only current-user policy");
    verify_current_user_access(&file, false, true).expect("read-only current-user DACL");
    let read_only = basic_info(&file);
    assert_eq!(read_only.FileAttributes & retained, retained);
    assert_ne!(read_only.FileAttributes & FILE_ATTRIBUTE_READONLY, 0);
    assert_eq!(read_only.CreationTime, before.CreationTime);
    assert_eq!(read_only.LastAccessTime, before.LastAccessTime);
    assert_eq!(read_only.LastWriteTime, before.LastWriteTime);
    assert!(
        OpenOptions::new().write(true).open(&path).is_err(),
        "read-only attribute prevents opening a new write handle",
    );

    apply_current_user_access(&file, false, false).expect("restore current-user-only DACL");
    verify_current_user_access(&file, false, false).expect("restored current-user-only DACL");
    let current_user_only = basic_info(&file);
    assert_ne!(current_user_only.FileAttributes & FILE_ATTRIBUTE_READONLY, 0);
    assert_eq!(current_user_only.CreationTime, read_only.CreationTime);
    assert_eq!(current_user_only.LastAccessTime, read_only.LastAccessTime);
    assert_eq!(current_user_only.LastWriteTime, read_only.LastWriteTime);
    assert!(
        OpenOptions::new().write(true).open(&path).is_err(),
        "CurrentUserOnly restores the DACL but does not clear FILE_ATTRIBUTE_READONLY",
    );
}

#[cfg(windows)]
#[test]
fn test_review_windows_owner_policy_preserves_preexisting_readonly_attribute() {
    let (directory, parent) = test_parent();
    let file = create_private_file_at(&parent, OsStr::new("preexisting-readonly"))
        .expect("create current-user-only file");
    let path = directory.path().join("preexisting-readonly");

    let mut information = basic_info(&file);
    information.FileAttributes |= FILE_ATTRIBUTE_READONLY;
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&information as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "set pre-existing FILE_ATTRIBUTE_READONLY",
    );
    assert_ne!(
        basic_info(&file).FileAttributes & FILE_ATTRIBUTE_READONLY,
        0
    );

    apply_current_user_access(&file, false, true).expect("apply read-only owner policy");
    verify_current_user_access(&file, false, true).expect("verify read-only owner DACL");
    apply_current_user_access(&file, false, false).expect("restore current-user-only owner policy");
    verify_current_user_access(&file, false, false).expect("verify restored owner DACL");
    assert_ne!(
        basic_info(&file).FileAttributes & FILE_ATTRIBUTE_READONLY,
        0,
        "restoring the DACL must preserve the pre-existing read-only attribute",
    );
    assert!(
        OpenOptions::new().write(true).open(&path).is_err(),
        "the preserved read-only attribute must continue to deny a write handle",
    );

    let mut information = basic_info(&file);
    information.FileAttributes &= !FILE_ATTRIBUTE_READONLY;
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&information as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "clear the read-only attribute for fixture cleanup",
    );
    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("clearing the attribute restores write access");
}

#[cfg(windows)]
#[test]
fn test_review_windows_access_transition_does_not_set_readonly_on_directories() {
    let (_directory, parent) = test_parent();
    let directory = create_private_directory_at(&parent, OsStr::new("access-transition-directory"))
        .expect("create current-user-only directory");

    apply_current_user_access(&directory, true, true).expect("apply read-only current-user policy");
    verify_current_user_access(&directory, true, true).expect("read-only current-user directory DACL");
    assert_eq!(basic_info(&directory).FileAttributes & FILE_ATTRIBUTE_READONLY, 0);

    apply_current_user_access(&directory, true, false).expect("apply current-user-only directory policy");
    verify_current_user_access(&directory, true, false).expect("current-user-only directory DACL");
    assert_eq!(basic_info(&directory).FileAttributes & FILE_ATTRIBUTE_READONLY, 0);
}
