// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::os::unix::fs::symlink;
use std::time::Duration;

use crate::LocalCopyConflictPolicy;
use crate::LocalCopyOptions;
use crate::LocalCopyTypeConflictPolicy;
use crate::LocalFileSystem;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;
use crate::tests::rooted::support::resolution_observation_tests as observations;
use crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary;

fn tree_replace_options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_tree_source()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_deadline(Duration::from_secs(60))
}

fn link_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).expect("source directory should exist");
    fs::create_dir(&target).expect("target directory should exist");
    fs::write(fixture.path().join("referent"), b"referent bytes").expect("referent should exist");
    symlink("../referent", source.join("link")).expect("source link should exist");
    (fixture, source, target)
}

fn assert_host_tree_symlink_create_failure_effect(replaced: bool) {
    let (_fixture, source, target) = link_fixture();
    let destination = target.join("link");
    if replaced {
        fs::write(&destination, b"old destination").expect("old destination");
    }
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::HostTreeSymlinkCreate, 1);
    let failure = LocalFileSystem::host()
        .expect("host filesystem")
        .copy_with_options(&source, &target, &tree_replace_options())
        .expect_err("injected link creation failure");
    assert!(guard.fired(), "injected symlink creation failure should fire");
    let expected = if replaced {
        crate::LocalCopyFailureState::PartiallyPublished
    } else {
        crate::LocalCopyFailureState::Unchanged
    };
    assert_eq!(expected, failure.state());
    assert_eq!(
        std::io::ErrorKind::NotFound,
        fs::symlink_metadata(&destination)
            .expect_err("destination absent")
            .kind()
    );
    assert_eq!(
        std::path::Path::new("../referent"),
        fs::read_link(source.join("link")).expect("source link retained")
    );
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().directories());
    assert_eq!(0, failure.partial_stats().bytes());
}

#[test]
fn test_host_tree_symlink_create_failure_after_removal_reports_partial() {
    assert_host_tree_symlink_create_failure_effect(true);
}

#[test]
fn test_host_tree_symlink_create_failure_without_removal_reports_unchanged() {
    assert_host_tree_symlink_create_failure_effect(false);
}

#[test]
fn test_rlf5_host_tree_symlink_target_read_timeout_preserves_old_destination() {
    let (_fixture, source, target) = link_fixture();
    let destination = target.join("link");
    fs::write(&destination, b"old destination").expect("old destination should exist");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostTreeSymlinkTargetRead);

    let failure = LocalFileSystem::host()
        .expect("host filesystem should be available")
        .copy_with_options(&source, &target, &tree_replace_options())
        .expect_err("expired tree link copy must stop before replacing the destination");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(
        b"old destination",
        fs::read(&destination).expect("old destination remains").as_slice()
    );
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(
        "../referent",
        fs::read_link(source.join("link"))
            .expect("source link remains")
            .to_string_lossy()
    );
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
}

#[test]
fn test_rlf5_host_tree_symlink_destination_removed_timeout_reports_partial() {
    let (_fixture, source, target) = link_fixture();
    let destination = target.join("link");
    fs::write(&destination, b"old destination").expect("old destination should exist");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostTreeSymlinkDestinationRemoved);

    let failure = LocalFileSystem::host()
        .expect("host filesystem should be available")
        .copy_with_options(&source, &target, &tree_replace_options())
        .expect_err("expired tree link copy must stop after removing the destination");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::PartiallyPublished, failure.state());
    assert_eq!(
        std::io::ErrorKind::NotFound,
        fs::symlink_metadata(&destination)
            .expect_err("destination should be absent")
            .kind()
    );
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
}

#[test]
fn test_rlf5_host_tree_symlink_removed_conflict_directory_timeout_reports_partial() {
    let (_fixture, source, target) = link_fixture();
    let destination = target.join("link");
    fs::create_dir(&destination).expect("conflicting destination directory should exist");
    fs::write(destination.join("child"), b"old child").expect("destination child should exist");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostTreeSymlinkDestinationRemoved);

    let failure = LocalFileSystem::host()
        .expect("host filesystem should be available")
        .copy_with_options(&source, &target, &tree_replace_options())
        .expect_err("expired tree link copy must stop after removing the directory");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::PartiallyPublished, failure.state());
    assert_eq!(
        std::io::ErrorKind::NotFound,
        fs::symlink_metadata(&destination)
            .expect_err("destination should be absent")
            .kind()
    );
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
}

#[test]
fn test_rlf5_host_tree_symlink_normal_copy_preserves_link_and_referent() {
    for options in [
        LocalCopyOptions::new().with_tree_source(),
        LocalCopyOptions::new()
            .with_tree_source()
            .with_deadline(Duration::from_secs(60)),
    ] {
        let (fixture, source, target) = link_fixture();
        let result = LocalFileSystem::host()
            .expect("host filesystem should be available")
            .copy_with_options(&source, &target, &options)
            .expect("tree link copy should succeed");

        assert_eq!(
            fs::read_link(source.join("link")).expect("source link should remain"),
            fs::read_link(target.join("link")).expect("destination link should be created"),
        );
        assert_eq!(
            b"referent bytes",
            fs::read(fixture.path().join("referent"))
                .expect("referent remains")
                .as_slice()
        );
        assert_eq!(1, result.stats().files());
        assert_eq!(0, result.stats().bytes());
    }
}
