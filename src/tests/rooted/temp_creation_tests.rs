// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalSymlinkPolicy;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTempFileOptions;
use crate::rooted_local_file_system::RootedLocalFileSystem;

#[test]
fn test_rooted_temp_backend_rejects_invalid_options_without_creating_entries() {
    let root = tempfile::tempdir().expect("root fixture should be created");
    let filesystem = RootedLocalFileSystem::open(root.path()).expect("root authority should open");
    let policy = LocalSymlinkPolicy::FollowWithinScope;
    for (file_options, directory_options) in [
        (
            LocalTempFileOptions::new().with_max_attempts(0),
            LocalTempDirectoryOptions::new().with_max_attempts(0),
        ),
        (
            LocalTempFileOptions::new().with_prefix("invalid/name"),
            LocalTempDirectoryOptions::new().with_prefix("invalid/name"),
        ),
        (
            LocalTempFileOptions::new().with_suffix("invalid/name"),
            LocalTempDirectoryOptions::new().with_suffix("invalid/name"),
        ),
    ] {
        let file_error = filesystem
            .create_temp_file(&file_options, policy)
            .expect_err("backend should independently reject invalid file options");
        assert_eq!(file_error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(file_error.operation(), LocalFileOperation::CreateTempFile);
        let directory_error = filesystem
            .create_temp_directory(&directory_options, policy)
            .expect_err("backend should independently reject invalid directory options");
        assert_eq!(directory_error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(directory_error.operation(), LocalFileOperation::CreateTempDirectory);
        assert_eq!(
            fs::read_dir(root.path()).expect("root should remain readable").count(),
            0
        );
    }
}

#[test]
fn test_rooted_temp_backend_retains_parent_errors_without_mutation() {
    let root = tempfile::tempdir().expect("root fixture should be created");
    fs::write(root.path().join("file"), b"keep").expect("blocking file should be written");
    let filesystem = RootedLocalFileSystem::open(root.path()).expect("root authority should open");
    let policy = LocalSymlinkPolicy::FollowWithinScope;
    for (parent, create_parent, expected) in [
        ("missing", false, LocalFileErrorKind::NotFound),
        ("file", false, LocalFileErrorKind::NotDirectory),
        ("file/child", true, LocalFileErrorKind::InvalidPath),
        ("../escape", false, LocalFileErrorKind::InvalidPath),
    ] {
        let mut file_options = LocalTempFileOptions::new().with_parent(Path::new(parent));
        let mut directory_options = LocalTempDirectoryOptions::new().with_parent(Path::new(parent));
        if create_parent {
            file_options = file_options.with_create_parent();
            directory_options = directory_options.with_create_parent();
        }
        let file_error = filesystem
            .create_temp_file(&file_options, policy)
            .expect_err("invalid file parent should fail");
        assert_eq!(file_error.kind(), expected, "file parent: {parent}");
        assert_eq!(file_error.operation(), LocalFileOperation::CreateTempFile);
        let directory_error = filesystem
            .create_temp_directory(&directory_options, policy)
            .expect_err("invalid directory parent should fail");
        assert_eq!(directory_error.kind(), expected, "directory parent: {parent}");
        assert_eq!(directory_error.operation(), LocalFileOperation::CreateTempDirectory);
        assert_eq!(
            fs::read(root.path().join("file")).expect("blocking file should remain"),
            b"keep"
        );
        assert_eq!(
            fs::read_dir(root.path()).expect("root should remain readable").count(),
            1
        );
    }
}

#[test]
fn test_rooted_temp_backend_creates_nested_parents_and_cleans_sandboxes() {
    let root = tempfile::tempdir().expect("root fixture should be created");
    let filesystem = RootedLocalFileSystem::open(root.path()).expect("root authority should open");
    let policy = LocalSymlinkPolicy::FollowWithinScope;
    let mut file = filesystem
        .create_temp_file(
            &LocalTempFileOptions::new()
                .with_parent(Path::new("files/nested"))
                .with_create_parent()
                .without_max_attempts(),
            policy,
        )
        .expect("backend should create missing file parents");
    assert!(root.path().join("files/nested").is_dir());
    assert_eq!(file.creation_parent(), Path::new("files/nested"));
    file.cleanup().expect("temporary file and sandbox should clean up");
    assert_eq!(
        fs::read_dir(root.path().join("files/nested"))
            .expect("parent should remain")
            .count(),
        0
    );
    let mut directory = filesystem
        .create_temp_directory(
            &LocalTempDirectoryOptions::new()
                .with_parent(Path::new("directories/nested"))
                .with_create_parent()
                .without_max_attempts(),
            policy,
        )
        .expect("backend should create missing directory parents");
    assert!(root.path().join("directories/nested").is_dir());
    assert_eq!(directory.creation_parent(), Path::new("directories/nested"));
    directory
        .cleanup()
        .expect("temporary directory and sandbox should clean up");
    assert_eq!(
        fs::read_dir(root.path().join("directories/nested"))
            .expect("parent should remain")
            .count(),
        0
    );
}
