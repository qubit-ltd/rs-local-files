// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local tests for rooted filesystem facade operations.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::LocalCreateDirectoryOptions;
use crate::LocalSymlinkPolicy;
use crate::rooted_local_file_system::RootedLocalFileSystem;

#[test]
fn test_rooted_recursive_directory_creation_and_root_metadata() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");

    let root_metadata = filesystem
        .metadata(Path::new(""), LocalSymlinkPolicy::FollowWithinScope)
        .expect("root metadata should be available");
    assert_eq!(crate::LocalFileKind::Directory, root_metadata.kind());

    let created = filesystem
        .create_directory(
            Path::new("nested/child"),
            &LocalCreateDirectoryOptions::new().with_recursive(),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect("missing rooted directory components should be created");
    assert!(created.created());
    assert!(directory.path().join("nested/child").is_dir());
}

#[test]
fn test_rooted_directory_creation_accepts_existing_directory() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("existing")).expect("fixture directory should be created");
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");

    let outcome = filesystem
        .create_directory(
            Path::new("existing"),
            &LocalCreateDirectoryOptions::new().with_exists_ok(),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect("existing directory should be accepted when requested");
    assert!(!outcome.created());
}

#[test]
fn test_rooted_metadata_reports_missing_entry() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");

    let error = filesystem
        .metadata(Path::new("missing"), LocalSymlinkPolicy::FollowWithinScope)
        .expect_err("missing entry metadata should fail");

    assert_eq!(crate::LocalFileOperation::Metadata, error.operation());
}

#[cfg(feature = "test-support")]
#[test]
fn test_rooted_recursive_directory_reports_partial_creation_failure() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");
    let _fault = crate::test_support::install_test_fault("rooted-create-directory-component-second")
        .expect("fault selector should install");

    let error = filesystem
        .create_directory(
            Path::new("created/blocked"),
            &LocalCreateDirectoryOptions::new().with_recursive(),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect_err("injected second component failure should be returned");

    assert_eq!(crate::LocalFileOperation::CreateDirectory, error.operation());
    assert!(directory.path().join("created").is_dir());
    assert!(!directory.path().join("created/blocked").exists());
}
