// =============================================================================

#![cfg(coverage)]
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Verifies file removal uses the responsibility module.
#[test]
fn test_remove_file_removes_existing_file() {
    let file =
        tempfile::NamedTempFile::new().expect("a temporary file should exist");
    let path = file
        .into_temp_path()
        .keep()
        .expect("the path should be retained");
    qubit_local_files::remove::file(&path).expect("the file should be removed");
}

/// Verifies directory and type-directed removal entry points.
#[test]
fn test_remove_directory_entry_points() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");

    let empty = directory.path().join("empty");
    std::fs::create_dir(&empty).expect("the empty directory should be created");
    qubit_local_files::remove::empty_directory(&empty)
        .expect("the empty directory should be removed");

    let tree = directory.path().join("tree");
    std::fs::create_dir(&tree).expect("the tree should be created");
    std::fs::write(tree.join("payload"), b"payload")
        .expect("the tree fixture should be written");
    qubit_local_files::remove::directory_tree(&tree)
        .expect("the tree should be removed");

    let any = directory.path().join("any");
    std::fs::write(&any, b"payload").expect("the file should be written");
    qubit_local_files::remove::any(&any)
        .expect("type-directed removal should remove the file");
}
