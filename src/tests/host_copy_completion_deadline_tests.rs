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
use crate::LocalCopyDirStage;
use crate::LocalCopyOptions;
use crate::LocalCopyTypeConflictPolicy;
use crate::LocalFileSystem;
use crate::local::copy_dir_all_with_paths;
use crate::tests::rooted::support::resolution_observation_tests as observations;
use crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary;

fn host_copy_directory_deadline_fixture(
    with_conflicting_file: bool,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let root = fs::canonicalize(fixture.path()).expect("canonicalize fixture directory");
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::create_dir(source.join("child")).unwrap();
    if with_conflicting_file {
        fs::write(target.join("child"), b"old").unwrap();
    }
    (fixture, source, target)
}

fn copy_directory_with_deadline(source: &std::path::Path, target: &std::path::Path) -> crate::LocalCopyFailure {
    LocalFileSystem::host()
        .unwrap()
        .copy_with_options(
            source,
            target,
            &LocalCopyOptions::new()
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired directory preparation must fail")
}

#[test]
fn test_host_copy_directory_deadline_after_source_inspection_preserves_target() {
    let (_fixture, source, target) = host_copy_directory_deadline_fixture(true);
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyChildDirectorySourceInspected);

    let failure = copy_directory_with_deadline(&source, &target);

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(Some(source.join("child").as_path()), failure.failed_source_path());
    assert_eq!(Some(target.join("child").as_path()), failure.failed_target_path());
    assert_eq!(b"old", fs::read(target.join("child")).unwrap().as_slice());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().directories());
}

#[test]
fn test_host_copy_directory_deadline_after_destination_inspection_preserves_target() {
    let (_fixture, source, target) = host_copy_directory_deadline_fixture(true);
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyDirectoryDestinationInspected);

    let failure = copy_directory_with_deadline(&source, &target);

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(b"old", fs::read(target.join("child")).unwrap().as_slice());
}

#[test]
fn test_host_copy_directory_deadline_after_destination_reinspection_preserves_target() {
    let (_fixture, source, target) = host_copy_directory_deadline_fixture(true);
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyDirectoryDestinationReinspected);

    let failure = copy_directory_with_deadline(&source, &target);

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(b"old", fs::read(target.join("child")).unwrap().as_slice());
}

#[test]
fn test_host_copy_directory_deadline_before_create_after_removal_is_partial() {
    let (_fixture, source, target) = host_copy_directory_deadline_fixture(true);
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyDirectoryBeforeCreate);

    let failure = copy_directory_with_deadline(&source, &target);

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::PartiallyPublished, failure.state());
    assert_eq!(Some(source.join("child").as_path()), failure.failed_source_path());
    assert_eq!(Some(target.join("child").as_path()), failure.failed_target_path());
    assert!(!target.join("child").exists());
    assert_eq!(0, failure.partial_stats().files());
}

#[test]
fn test_host_copy_directory_deadline_before_create_without_prior_mutation_is_unchanged() {
    let (_fixture, source, target) = host_copy_directory_deadline_fixture(false);
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyDirectoryBeforeCreate);

    let failure = copy_directory_with_deadline(&source, &target);

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
    assert!(!target.join("child").exists());
}

#[test]
fn test_host_copy_directory_without_deadline_still_replaces_conflicting_file() {
    let (_fixture, source, target) = host_copy_directory_deadline_fixture(true);

    let _outcome = LocalFileSystem::host()
        .unwrap()
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new().with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect("copy without a deadline should replace the conflicting file");

    assert!(target.join("child").is_dir());
}

#[test]
fn test_host_copy_deadline_after_staging_preserves_existing_target() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("item"), b"new").unwrap();
    fs::write(target.join("item"), b"old").unwrap();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyBeforeInstall);

    let failure = LocalFileSystem::host()
        .unwrap()
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline before install must fail");

    assert_eq!(b"old", fs::read(target.join("item")).unwrap().as_slice());
    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
    assert_eq!(
        vec!["item"],
        fs::read_dir(&target)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
}

#[test]
fn test_host_copy_deadline_after_data_eof_cleans_staging() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("item"), b"new").unwrap();
    fs::write(target.join("item"), b"old").unwrap();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyDataFinished);

    let failure = LocalFileSystem::host()
        .unwrap()
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline after data EOF must fail");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(b"old", fs::read(target.join("item")).unwrap().as_slice());
    assert_eq!(
        vec!["item"],
        fs::read_dir(&target)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(crate::LocalCopyFailureState::Unchanged, failure.state());
}

#[test]
fn test_host_copy_deadline_before_final_directory_metadata_stops_frame_finish() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyBeforeDirectoryMetadata);

    let failure = copy_dir_all_with_paths(
        &source,
        &target,
        LocalCopyDirOptions::new()
            .preserve_permissions()
            .with_deadline(Duration::from_secs(60)),
    )
    .expect_err("deadline at directory EOF must stop before metadata update");

    assert_eq!(LocalCopyDirStage::PreservePermissions, failure.stage());
    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().kind());
    assert_eq!(0, failure.stats().files());
}

#[test]
fn test_host_copy_deadline_after_conflict_directory_removal_is_partial() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let root = fs::canonicalize(fixture.path()).expect("canonicalize fixture directory");
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("item"), b"new").unwrap();
    fs::create_dir(target.join("item")).unwrap();
    fs::write(target.join("item/old"), b"old").unwrap();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyAfterConflictRemoval);

    let failure = copy_dir_all_with_paths(
        &source,
        &target,
        LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
            .with_deadline(Duration::from_secs(60)),
    )
    .expect_err("deadline after conflict removal must fail");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().kind());
    assert_eq!(
        Some(crate::LocalCopyFailureState::PartiallyPublished),
        failure.failure_state()
    );
    assert!(!target.join("item").exists());
    assert_eq!(0, failure.stats().files());
}

#[test]
fn test_host_copy_deadline_after_published_child_keeps_partial_stats() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("item"), b"new").unwrap();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopyBeforeDirectoryMetadata);

    let failure = copy_dir_all_with_paths(
        &source,
        &target,
        LocalCopyDirOptions::new().with_deadline(Duration::from_secs(60)),
    )
    .expect_err("deadline after child publication must report partial progress");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().kind());
    assert_eq!(b"new", fs::read(target.join("item")).unwrap().as_slice());
    assert_eq!(1, failure.stats().files());
    assert_eq!(3, failure.stats().bytes());
}

#[test]
fn test_host_copy_deadline_before_second_install_preserves_first_item_progress() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("alpha"), b"one").unwrap();
    fs::write(source.join("omega"), b"two").unwrap();
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::HostCopySecondBeforeInstall);

    let failure = copy_dir_all_with_paths(
        &source,
        &target,
        LocalCopyDirOptions::new().with_deadline(Duration::from_secs(60)),
    )
    .expect_err("second install must observe the deadline");

    assert_eq!(std::io::ErrorKind::TimedOut, failure.error().kind());
    assert_eq!(1, failure.stats().files());
    assert_eq!(3, failure.stats().bytes());
    assert_eq!(
        1,
        [target.join("alpha").exists(), target.join("omega").exists()]
            .into_iter()
            .filter(|exists| *exists)
            .count(),
    );
    assert_eq!(
        1,
        fs::read_dir(&target).unwrap().count(),
        "the target contains one published item and no staging residue",
    );
}
