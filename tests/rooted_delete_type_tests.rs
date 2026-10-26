// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDeleteOptions;
use tempfile::tempdir;

#[test]
fn test_rooted_delete_type_contract_preserves_mismatched_entries() {
    let directory = tempdir().expect("fixture directory should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let file = directory.path().join("file");
    let nonempty = directory.path().join("directory");
    fs::write(&file, b"keep").expect("file should be written");
    fs::create_dir(&nonempty).expect("directory should be created");
    fs::write(nonempty.join("child"), b"keep").expect("directory child should be written");

    let file_as_directory = filesystem
        .delete_directory_with_options(Path::new("file"), &LocalDeleteOptions::new())
        .expect_err("directory deletion must reject a file");
    assert_eq!(LocalFileErrorKind::NotDirectory, file_as_directory.kind());
    assert_eq!(b"keep", fs::read(&file).expect("file should remain").as_slice());

    let directory_as_file = filesystem
        .delete_file_with_options(Path::new("directory"), &LocalDeleteOptions::new())
        .expect_err("file deletion must reject a real directory");
    assert_eq!(LocalFileErrorKind::IsDirectory, directory_as_file.kind());
    assert!(nonempty.join("child").exists());

    let nonempty_error = filesystem
        .delete_directory_with_options(Path::new("directory"), &LocalDeleteOptions::new())
        .expect_err("empty-directory deletion must reject a nonempty directory");
    assert!(fs::read(nonempty.join("child")).is_ok());
    assert!(nonempty_error.io_error().is_some());

    let deleted = filesystem
        .delete_file_with_options(Path::new("file"), &LocalDeleteOptions::new())
        .expect("file deletion should succeed");
    assert!(deleted.deleted());
    assert!(!file.exists());
}
