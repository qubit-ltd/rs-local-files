// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use crate::LocalFileSystem;
use crate::options::LocalTempDirectoryOptions;

fn temporary_directory(parent: &Path) -> crate::LocalTempDirectory {
    LocalFileSystem::host()
        .expect("host filesystem")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(parent))
        .expect("temporary directory")
}

#[test]
fn test_temp_directory_descendant_validates_and_joins_nested_paths() {
    let parent = tempfile::tempdir().expect("parent fixture");
    let temporary = temporary_directory(parent.path());
    let path = temporary.path().to_path_buf();

    let descendant = temporary
        .descendant(Path::new("nested/file.txt"))
        .expect("valid descendant");
    assert_eq!(descendant, path.join("nested/file.txt"));
    assert_eq!(fs::read_dir(parent.path()).expect("parent entries").count(), 1);
    assert!(temporary.descendant(Path::new("../outside")).is_err());
    assert!(temporary.descendant(Path::new("")).is_err());

    drop(temporary);
    assert!(!path.exists(), "dropping the owner removes the temporary directory");
    assert_eq!(fs::read_dir(parent.path()).expect("parent entries").count(), 0);
}

#[test]
fn test_temp_directory_cleanup_removes_tree_and_is_idempotent() {
    let parent = tempfile::tempdir().expect("parent fixture");
    let mut temporary = temporary_directory(parent.path());
    let path = temporary.path().to_path_buf();
    let file = temporary
        .descendant(Path::new("nested/file.txt"))
        .expect("valid descendant");
    fs::create_dir_all(file.parent().expect("file parent")).expect("nested directory");
    fs::write(&file, b"temporary contents").expect("temporary file");

    temporary.cleanup().expect("explicit cleanup");
    assert!(!path.exists(), "cleanup removes the directory tree");
    assert_eq!(fs::read_dir(parent.path()).expect("parent entries").count(), 0);
    temporary.cleanup().expect("released cleanup is idempotent");
}

#[test]
fn test_temp_directory_persist_publishes_tree_and_releases_sandbox() {
    let parent = tempfile::tempdir().expect("parent fixture");
    let temporary = temporary_directory(parent.path());
    let source = temporary.path().to_path_buf();
    let file = temporary
        .descendant(Path::new("nested/file.txt"))
        .expect("valid descendant");
    fs::create_dir_all(file.parent().expect("file parent")).expect("nested directory");
    fs::write(&file, b"published contents").expect("temporary file");
    let target = parent.path().join("published");

    let outcome = temporary.persist(&target).expect("persist without replacement");
    assert_eq!(outcome.path(), target);
    assert!(outcome.atomic());
    assert!(!source.exists(), "publication moves the source directory");
    assert_eq!(
        fs::read(target.join("nested/file.txt")).expect("published file"),
        b"published contents"
    );
    assert_eq!(fs::read_dir(parent.path()).expect("parent entries").count(), 1);
}
