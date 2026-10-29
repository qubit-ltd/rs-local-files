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
use crate::LocalListOptions;
use crate::LocalSymlinkPolicy;
#[cfg(unix)]
use crate::LocalWalkErrorPolicy;
use crate::rooted_local_file_system::RootedLocalFileSystem;

#[test]
fn test_rooted_listing_rejects_an_unrepresentable_backend_deadline() {
    let fixture = tempdir().expect("fixture should exist");
    let filesystem = RootedLocalFileSystem::open(fixture.path()).expect("rooted authority should open");
    let error = filesystem
        .list(
            Path::new(""),
            Path::new("/"),
            &LocalListOptions::new().with_deadline(std::time::Duration::MAX),
            LocalSymlinkPolicy::FollowWithinScope,
            std::time::Instant::now(),
        )
        .expect_err("an unrepresentable backend deadline must fail before opening a walker");
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), crate::error::LocalFileOperation::List);
    assert_eq!(error.path(), Some(Path::new("")));
    assert_eq!(error.reason(), Some("listing deadline is not representable"));
    assert_eq!(
        fs::read_dir(fixture.path())
            .expect("root should remain unchanged")
            .count(),
        0
    );
}

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

#[cfg(unix)]
#[test]
fn test_rooted_branching_resolution_observes_deadline() {
    use std::os::unix::fs::symlink;
    use std::time::Duration;
    use std::time::Instant;

    let directory = tempdir().expect("temporary directory should be created");
    symlink(".", directory.path().join("L0")).expect("base link should be created");
    for index in 1..=8 {
        let previous = format!("L{}", index - 1);
        symlink(
            format!("{previous}/{previous}"),
            directory.path().join(format!("L{index}")),
        )
        .expect("branching link should be created");
    }
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");
    crate::tests::rooted::support::resolution_observation_tests::reset();
    let _clock = crate::tests::rooted::support::resolution_observation_tests::DeadlineClockGuard::expire_after(8);
    let error = filesystem
        .list(
            Path::new("L8"),
            Path::new("/"),
            &LocalListOptions::new().with_deadline(Duration::from_secs(60)),
            LocalSymlinkPolicy::FollowWithinScope,
            Instant::now(),
        )
        .expect_err("resolution should expire during link expansion");

    assert_eq!(crate::LocalFileOperation::List, error.operation());
    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
    assert!(crate::tests::rooted::support::resolution_observation_tests::snapshot().2 > 0);
}

#[test]
fn test_rooted_cursor_deadline_does_not_fall_back() {
    use std::time::Duration;
    use std::time::Instant;

    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir_all(directory.path().join("a/b/c")).expect("nested directories should be created");
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");
    crate::tests::rooted::support::resolution_observation_tests::reset();
    let _clock = crate::tests::rooted::support::resolution_observation_tests::DeadlineClockGuard::expire_after(4);

    let error = filesystem
        .list(
            Path::new("a/b/c"),
            Path::new("/"),
            &LocalListOptions::new().with_deadline(Duration::from_secs(60)),
            LocalSymlinkPolicy::FollowWithinScope,
            Instant::now(),
        )
        .expect_err("cursor resolution should expire");

    assert_eq!(crate::LocalFileOperation::List, error.operation());
    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
    assert_eq!(
        0,
        crate::tests::rooted::support::resolution_observation_tests::snapshot().2
    );
}

#[test]
fn test_rooted_empty_list_checks_deadline_after_root_metadata() {
    use std::time::Duration;
    use std::time::Instant;

    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");
    let _clock = crate::tests::rooted::support::resolution_observation_tests::DeadlineClockGuard::expire_after(1);

    let error = filesystem
        .list(
            Path::new(""),
            Path::new("/"),
            &LocalListOptions::new().with_deadline(Duration::from_secs(60)),
            LocalSymlinkPolicy::FollowWithinScope,
            Instant::now(),
        )
        .expect_err("root metadata completion should be followed by a deadline check");

    assert_eq!(crate::LocalFileOperation::List, error.operation());
    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
}

#[cfg(unix)]
#[test]
fn test_rooted_list_start_resolves_once_and_finite_branching_without_deadline() {
    use std::os::unix::fs::symlink;
    use std::time::Instant;

    let directory = tempdir().expect("temporary directory should be created");
    symlink(".", directory.path().join("L0")).expect("base link should be created");
    for index in 1..=4 {
        let previous = format!("L{}", index - 1);
        symlink(
            format!("{previous}/{previous}"),
            directory.path().join(format!("L{index}")),
        )
        .expect("branching link should be created");
    }
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");
    crate::tests::rooted::support::resolution_observation_tests::reset();
    let walker = filesystem
        .list(
            Path::new("L4"),
            Path::new("/"),
            &LocalListOptions::new(),
            LocalSymlinkPolicy::FollowWithinScope,
            Instant::now(),
        )
        .expect("finite branching links should resolve without a deadline");
    drop(walker);
    assert_eq!(
        1,
        crate::tests::rooted::support::resolution_observation_tests::snapshot().2
    );
}

#[cfg(unix)]
#[test]
fn test_rooted_child_resolution_timeout_terminates_continue() {
    use std::os::unix::fs::symlink;
    use std::time::Duration;
    use std::time::Instant;

    let directory = tempdir().expect("temporary directory should be created");
    symlink(".", directory.path().join("L0")).expect("base link should be created");
    for index in 1..=8 {
        let previous = format!("L{}", index - 1);
        symlink(
            format!("{previous}/{previous}"),
            directory.path().join(format!("L{index}")),
        )
        .expect("branching link should be created");
    }
    let filesystem = RootedLocalFileSystem::open(directory.path()).expect("rooted filesystem should open");
    let mut walker = filesystem
        .list(
            Path::new(""),
            Path::new("/"),
            &LocalListOptions::new()
                .with_recursive()
                .with_error_policy(LocalWalkErrorPolicy::Continue)
                .with_deadline(Duration::from_secs(60)),
            LocalSymlinkPolicy::FollowWithinScope,
            Instant::now(),
        )
        .expect("root listing should open before the controlled expiry");
    let _clock = crate::tests::rooted::support::resolution_observation_tests::DeadlineClockGuard::expire_after(8);

    let error = walker
        .next()
        .expect("resolution should return an error")
        .expect_err("deadline should expire");
    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
    assert!(
        walker.next().is_none(),
        "Continue must terminate after a deadline error"
    );
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
