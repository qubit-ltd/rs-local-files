// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDeleteOptions;
use tempfile::tempdir;

/// A recursive directory request must reject a regular file without deleting
/// it, consistently for Host and Rooted authorities.
#[test]
fn host_recursive_directory_delete_rejects_regular_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let physical = directory.path().join("victim");
    std::fs::write(&physical, b"must survive").expect("victim fixture should be written");

    let error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_directory_with_options(&physical, &LocalDeleteOptions::new().with_recursive())
        .expect_err("a regular file is not a directory");
    assert_eq!(LocalFileErrorKind::NotDirectory, error.kind());
    assert_eq!(std::fs::read(&physical).expect("victim must survive"), b"must survive");
}

#[test]
fn rooted_recursive_directory_delete_rejects_regular_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let physical = directory.path().join("victim");
    std::fs::write(&physical, b"must survive").expect("victim fixture should be written");

    let error = LocalFileSystem::rooted(directory.path())
        .expect("root authority should open")
        .delete_directory_with_options(Path::new("victim"), &LocalDeleteOptions::new().with_recursive())
        .expect_err("a regular file is not a directory");
    assert_eq!(LocalFileErrorKind::NotDirectory, error.kind());
    assert_eq!(std::fs::read(&physical).expect("victim must survive"), b"must survive");
}
