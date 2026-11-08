// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::os::windows::fs::symlink_dir;

use tempfile::tempdir;

use crate::local::internal::remove_host_non_directory;

#[test]
fn test_remove_host_non_directory_rejects_replaced_directory_symlink() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let entry = directory.path().join("entry");
    fs::create_dir(&target).expect("link target should be created");
    symlink_dir(&target, &entry).expect("directory link should be created");
    let metadata = fs::symlink_metadata(&entry).expect("link metadata should be captured");

    fs::remove_dir(&entry).expect("link should be replaced");
    fs::create_dir(&entry).expect("replacement directory should be created");

    let result = remove_host_non_directory(&entry, &metadata);

    assert!(result.is_err(), "replacement directory must not be removed");
    assert!(entry.is_dir(), "replacement directory should remain");
}
