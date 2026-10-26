// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local contract tests for Host directory creation.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::LocalCreateDirectoryOptions;
use crate::LocalSymlinkPolicy;
use crate::local::HostLocalFileSystem;

#[test]
fn test_host_recursive_directory_creation_reports_new_target() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("nested/child");

    let outcome = HostLocalFileSystem::create_directory_with_policy(
        &target,
        &LocalCreateDirectoryOptions::new().with_recursive(),
        LocalSymlinkPolicy::FollowAcrossScope,
    )
    .expect("recursive directory creation should succeed");

    assert!(outcome.created());
    assert!(target.is_dir());
}

#[test]
fn test_host_directory_creation_rejects_file_component() {
    let directory = tempdir().expect("temporary directory should be created");
    let blocker = directory.path().join("blocker");
    fs::write(&blocker, b"file").expect("blocking file should be written");

    let error = HostLocalFileSystem::create_directory_with_policy(
        Path::new(&blocker).join("child").as_path(),
        &LocalCreateDirectoryOptions::new().with_recursive(),
        LocalSymlinkPolicy::FollowAcrossScope,
    )
    .expect_err("a file cannot be used as a directory ancestor");

    assert_eq!(crate::LocalFileOperation::BindPath, error.operation());
}

#[cfg(feature = "test-support")]
#[test]
fn test_host_recursive_directory_reports_partial_creation_failure() {
    let directory = tempdir().expect("temporary directory should be created");
    let _fault = crate::test_support::install_test_fault("host-create-directory-component-second")
        .expect("fault selector should install");

    let error = HostLocalFileSystem::create_directory_with_policy(
        &directory.path().join("created/blocked"),
        &LocalCreateDirectoryOptions::new().with_recursive(),
        LocalSymlinkPolicy::FollowAcrossScope,
    )
    .expect_err("injected second component failure should be returned");

    assert_eq!(crate::LocalFileOperation::CreateDirectory, error.operation());
    assert!(directory.path().join("created").is_dir());
    assert!(!directory.path().join("created/blocked").exists());
}
