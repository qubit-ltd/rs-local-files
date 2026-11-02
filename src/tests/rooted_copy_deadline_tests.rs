// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::time::Duration;

use crate::LocalCopyConflictPolicy;
use crate::LocalCopyDirOptions;
use crate::LocalCopyOptions;
use crate::LocalCopyTypeConflictPolicy;
use crate::LocalFileSystem;
use crate::local::CopyBudget;
use crate::tests::rooted::support::resolution_observation_tests as observations;
use crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary;

#[test]
fn test_rooted_copy_deadline_after_source_resolution_keeps_parent_absent() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"payload").expect("source should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    observations::reset();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::SourceResolved);

    let failure = filesystem
        .copy_with_options(
            std::path::Path::new("source"),
            std::path::Path::new("new-parent/target"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired resolution must stop before parent creation");

    assert_eq!(crate::LocalFileOperation::Copy, failure.error().operation());
    assert_eq!(crate::LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert!(!fixture.path().join("new-parent").exists());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
}

#[test]
fn test_rooted_copy_with_and_without_positive_deadline_succeeds() {
    for deadline in [None, Some(Duration::from_secs(60))] {
        let fixture = tempfile::tempdir().expect("fixture should exist");
        fs::write(fixture.path().join("source"), b"payload").expect("source should exist");
        let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
        let mut options = LocalCopyOptions::new();
        if let Some(deadline) = deadline {
            options = options.with_deadline(deadline);
        }

        let outcome = filesystem
            .copy_with_options(std::path::Path::new("source"), std::path::Path::new("target"), &options)
            .expect("ordinary rooted copy should succeed");

        assert_eq!(1, outcome.stats().files());
        assert_eq!(
            b"payload",
            fs::read(fixture.path().join("target"))
                .expect("copied target should exist")
                .as_slice()
        );
    }
}

#[test]
fn test_copy_budget_absolute_deadline_preserves_origin_and_rejects_overflow() {
    let no_deadline = CopyBudget::new(LocalCopyDirOptions::new());
    assert_eq!(
        None,
        no_deadline.absolute_deadline().expect("unbounded deadline is valid")
    );

    let started_at = std::time::Instant::now();
    let duration = Duration::from_secs(30);
    let budget = CopyBudget::new(
        LocalCopyDirOptions::new()
            .with_started_at(started_at)
            .with_deadline(duration),
    );
    assert_eq!(
        Some(started_at + duration),
        budget.absolute_deadline().expect("deadline should project")
    );

    let unrepresentable = CopyBudget::new(
        LocalCopyDirOptions::new()
            .with_started_at(started_at)
            .with_deadline(Duration::MAX),
    );
    assert_eq!(
        std::io::ErrorKind::InvalidInput,
        unrepresentable
            .absolute_deadline()
            .expect_err("overflow should be rejected")
            .kind()
    );
}

#[test]
fn test_rooted_copy_deadline_during_source_resolution_stops_native_observation() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir_all(fixture.path().join("a/b/c")).expect("nested source directory should exist");
    fs::write(fixture.path().join("a/b/c/source"), b"payload").expect("source should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    observations::reset();
    let _clock = observations::DeadlineClockGuard::expire_after(4);

    let failure = filesystem
        .copy_with_options(
            std::path::Path::new("a/b/c/source"),
            std::path::Path::new("target"),
            &LocalCopyOptions::new().with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline during source resolution must stop the copy");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert!(observations::deadline_check_count() > 4);
    assert!(
        observations::snapshot().0 < 3,
        "resolution must stop before observing all three intermediate components"
    );
    assert!(!fixture.path().join("target").exists());
}

#[test]
fn test_rooted_copy_deadline_after_target_resolution_keeps_parent_absent() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"payload").expect("source should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    observations::reset();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::TargetResolved);

    let failure = filesystem
        .copy_with_options(
            std::path::Path::new("source"),
            std::path::Path::new("new-parent/target"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired resolution must stop before parent creation");

    assert_eq!(crate::LocalFileOperation::Copy, failure.error().operation());
    assert_eq!(crate::LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert!(!fixture.path().join("new-parent").exists());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
}

#[test]
fn test_rooted_copy_deadline_after_removal_reports_partial_publication() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(fixture.path().join("source")).expect("source directory should exist");
    fs::write(fixture.path().join("source/child"), b"new").expect("source child should exist");
    fs::write(fixture.path().join("target"), b"old").expect("old target should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::DestinationRemoved);

    let failure = filesystem
        .copy_with_options(
            std::path::Path::new("source"),
            std::path::Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline after replacement removal must report partial publication");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(!fixture.path().join("target").exists());
}

#[cfg(windows)]
#[test]
fn test_rooted_link_kind_deadline_keeps_existing_file() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(fixture.path().join("source")).expect("source directory should exist");
    fs::create_dir(fixture.path().join("target")).expect("target directory should exist");
    std::os::windows::fs::symlink_file("missing-target", fixture.path().join("source/link"))
        .expect("source symbolic link should be created");
    assert_link_kind_deadline_keeps_existing_file(fixture.path());
}

#[cfg(unix)]
#[test]
fn test_rooted_symlink_copy_deadline_checkpoint_keeps_existing_file() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(fixture.path().join("source")).expect("source directory should exist");
    fs::create_dir(fixture.path().join("target")).expect("target directory should exist");
    std::os::unix::fs::symlink("missing-target", fixture.path().join("source/link"))
        .expect("source symbolic link should be created");
    assert_link_kind_deadline_keeps_existing_file(fixture.path());
}

fn assert_link_kind_deadline_keeps_existing_file(root: &std::path::Path) {
    fs::write(root.join("target/link"), b"protected").expect("preexisting target should exist");
    let filesystem = LocalFileSystem::rooted(root).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::RootedLinkKindInspected);

    let failure = filesystem
        .copy_with_options(
            std::path::Path::new("source"),
            std::path::Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired deadline after link inspection must stop before target removal");
    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
    assert_eq!(
        b"protected",
        fs::read(root.join("target/link"))
            .expect("preexisting target should remain")
            .as_slice()
    );
}

#[cfg(unix)]
#[test]
fn test_rooted_copy_deadline_after_followed_directory_resolution_keeps_target() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(fixture.path().join("source")).expect("source directory should exist");
    fs::create_dir(fixture.path().join("link-target")).expect("link target should exist");
    fs::write(fixture.path().join("link-target/old"), b"protected").expect("old target should exist");
    symlink("../link-target", fixture.path().join("source/link-dir")).expect("directory link should exist");
    fs::create_dir(fixture.path().join("target")).expect("destination should exist");
    fs::write(fixture.path().join("target/link-dir"), b"old type").expect("conflicting target should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    observations::reset();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::FollowedLinkResolved);

    let failure = filesystem
        .copy_with_options(
            std::path::Path::new("source"),
            std::path::Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_symlink_policy(crate::LocalSymlinkPolicy::FollowWithinScope)
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired link resolution must stop before replacing its destination");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(
        b"old type",
        fs::read(fixture.path().join("target/link-dir"))
            .expect("preexisting file should remain")
            .as_slice()
    );
    assert!(fixture.path().join("target/link-dir").is_file());
    assert_eq!(
        observations::metadata_count_at_boundary(),
        Some(observations::snapshot().0),
        "deadline expiration must prevent post-resolution source metadata observation",
    );
}
