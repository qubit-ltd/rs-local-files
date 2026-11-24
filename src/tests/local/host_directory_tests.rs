// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local contract tests for Host directory creation.

use std::fs;

use tempfile::tempdir;

use crate::LocalCreateDirectoryOptions;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
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

#[cfg(feature = "test-support")]
#[test]
fn test_host_directory_creation_accepts_concurrent_directory_creation() {
    let directory = tempdir().expect("temporary directory should be created");
    let host = HostLocalFileSystem::create_directory_with_policy;
    let target = directory.path().join("nonrecursive-race");
    let race = crate::test_support::install_test_fault("host-create-directory-target-race")
        .expect("target race should install");
    let outcome = host(
        &target,
        &LocalCreateDirectoryOptions::new().with_exists_ok(),
        LocalSymlinkPolicy::FollowAcrossScope,
    )
    .expect("an already-created target directory should count as success");
    assert!(!outcome.created());
    assert!(target.is_dir());
    drop(race);

    let target = directory.path().join("recursive-race");
    let race = crate::test_support::install_test_fault("host-create-directory-component-race")
        .expect("recursive component race should install");
    let outcome = host(
        &target,
        &LocalCreateDirectoryOptions::new().with_recursive().with_exists_ok(),
        LocalSymlinkPolicy::FollowAcrossScope,
    )
    .expect("an already-created recursive target directory should count as success");
    assert!(!outcome.created());
    assert!(target.is_dir());
    drop(race);

    let target = directory.path().join("raced-parent/child");
    let _race = crate::test_support::install_test_fault("host-create-directory-component-race")
        .expect("recursive parent race should install");
    let outcome = host(
        &target,
        &LocalCreateDirectoryOptions::new().with_recursive(),
        LocalSymlinkPolicy::FollowAcrossScope,
    )
    .expect("recursive creation should continue after an ancestor wins a race");
    assert!(outcome.created());
    assert!(target.is_dir());
}

#[test]
fn test_host_directory_creation_rejects_file_component() {
    let directory = tempdir().expect("temporary directory should be created");
    let blocker = directory.path().join("blocker");
    fs::write(&blocker, b"file").expect("blocking file should be written");

    let error = HostLocalFileSystem::create_directory_with_policy(
        &blocker.join("child"),
        &LocalCreateDirectoryOptions::new().with_recursive(),
        LocalSymlinkPolicy::FollowAcrossScope,
    )
    .expect_err("a file cannot be used as a directory ancestor");

    assert!(matches!(
        error.operation(),
        LocalFileOperation::BindPath | LocalFileOperation::CreateDirectory
    ));
}

#[cfg(unix)]
#[test]
fn test_host_recursive_directory_creation_rejects_intermediate_symlink_by_policy() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("real");
    fs::create_dir(&target).expect("real directory should be created");
    let alias = directory.path().join("alias");
    symlink(&target, &alias).expect("directory symlink should be created");
    let requested = alias.join("child");

    let error = HostLocalFileSystem::create_directory_with_policy(
        &requested,
        &LocalCreateDirectoryOptions::new().with_recursive(),
        LocalSymlinkPolicy::Reject,
    )
    .expect_err("Reject policy should block creation through an intermediate symlink");

    assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
    assert!(
        !target.join("child").exists(),
        "the symlink target must remain untouched"
    );
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

    assert_eq!(LocalFileOperation::CreateDirectory, error.operation());
    assert!(directory.path().join("created").is_dir());
    assert!(!directory.path().join("created/blocked").exists());
}
