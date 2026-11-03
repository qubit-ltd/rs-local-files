// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::cell::Cell;
use std::fs;
use std::io::ErrorKind;
use std::os::windows::fs::OpenOptionsExt;

use tempfile::tempdir;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::open_root_directory_reader_with_reopen;

#[test]
fn test_root_enumeration_fallback_fails_closed_without_independent_handle() {
    let fixture = tempdir().expect("directory fixture should be created");
    fs::write(fixture.path().join("entry"), []).expect("fixture entry should be created");
    let root = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(fixture.path())
        .expect("root directory handle should open");
    let fallback_count = Cell::new(0);

    let result = open_root_directory_reader_with_reopen(&root, |_| {
        fallback_count.set(fallback_count.get() + 1);
        Ok(None)
    });
    let error = match result {
        Ok(_) => panic!("fallback must not return a reader with a shared native cursor"),
        Err(error) => error,
    };

    assert_eq!(ErrorKind::Unsupported, error.kind());
    assert_eq!(1, fallback_count.get(), "reopen should be attempted once");
}
