// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::fs::File;
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ARCHIVE;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
use windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO;
use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;

use super::FILE_FLAG_BACKUP_SEMANTICS;
use super::apply_current_user_access;
use super::create_private_directory_at;
use super::create_private_file_at;
use super::verify_current_user_access;

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

fn test_parent() -> (tempfile::TempDir, File) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(directory.path())
        .expect("open temporary directory handle");
    (directory, parent)
}

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
