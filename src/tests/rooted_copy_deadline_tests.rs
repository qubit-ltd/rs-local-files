// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_file;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::LocalCopyConflictPolicy;
use crate::LocalCopyDirOptions;
use crate::LocalCopyFailureState;
use crate::LocalCopyOptions;
use crate::LocalCopyTypeConflictPolicy;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
#[cfg(unix)]
use crate::LocalSymlinkPolicy;
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
            Path::new("source"),
            Path::new("new-parent/target"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired resolution must stop before parent creation");

    assert_eq!(LocalFileOperation::Copy, failure.error().operation());
    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
    assert!(!fixture.path().join("new-parent").exists());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
}

#[test]
fn test_rooted_delete_deadline_after_path_resolution_skips_initial_metadata() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let target = fixture.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("child"), b"keep").unwrap();
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::RootedDeletePathResolved);

    let failure = filesystem
        .delete_directory_with_options(
            Path::new("target"),
            &crate::LocalDeleteOptions::new()
                .with_recursive()
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired deletion must stop before initial metadata inspection");

    assert_eq!(ErrorKind::TimedOut, failure.io_error_kind());
    assert_eq!(b"keep", fs::read(target.join("child")).unwrap().as_slice());
}

#[test]
fn test_rooted_copy_parent_preflight_deadline_prevents_parent_creation() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"source").unwrap();
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::RootedCopyParentPreflightComplete);

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("new-parent/child/target"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline after parent preflight must prevent creation");

    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert!(!fixture.path().join("new-parent").exists());
}

#[test]
fn test_rooted_copy_accepts_parent_created_after_preflight() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"payload").expect("source should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let mut expired_attempts = 0;
    let mut completed = false;
    let mut race_observed = false;

    for allowance in 0..64 {
        let parent = PathBuf::from(format!("parent-{allowance}"));
        let native_parent = fixture.path().join(&parent);
        let created_by_racer = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let callback_flag = std::sync::Arc::clone(&created_by_racer);
        let callback_parent = native_parent.clone();
        let _hook = observations::install_rooted_copy_parent_preflight_callback(move || {
            fs::create_dir(callback_parent).expect("concurrent parent creation should win the race");
            callback_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let _clock = observations::DeadlineClockGuard::expire_after(allowance);
        let target = parent.join("target");

        match filesystem.copy_with_options(
            Path::new("source"),
            &target,
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        ) {
            Ok(outcome) => {
                assert_eq!(1, outcome.stats().files());
                assert_eq!(7, outcome.stats().bytes());
                assert_eq!(b"payload", fs::read(fixture.path().join(&target)).unwrap().as_slice());
                completed = true;
                race_observed |= created_by_racer.load(std::sync::atomic::Ordering::SeqCst);
                break;
            }
            Err(failure) => {
                assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
                if created_by_racer.load(std::sync::atomic::Ordering::SeqCst) {
                    race_observed = true;
                    assert!(native_parent.is_dir());
                }
                match failure.state() {
                    LocalCopyFailureState::Unchanged | LocalCopyFailureState::PartiallyPublished => {
                        assert!(!fixture.path().join(&target).exists());
                    }
                    LocalCopyFailureState::Published => {
                        assert_eq!(b"payload", fs::read(fixture.path().join(&target)).unwrap().as_slice())
                    }
                    LocalCopyFailureState::Indeterminate => {}
                }
                expired_attempts += 1;
            }
        }
    }

    assert!(completed, "deadline sweep must eventually complete the copy");
    assert!(race_observed, "a parent must be created after rooted preflight");
    assert!(
        expired_attempts >= 6,
        "deadline sweep must cover the raced create check"
    );
}

#[test]
fn test_rooted_copy_parent_creation_deadline_reports_unchanged_target() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"source").unwrap();
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::RootedCopyParentCreated);

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("new-parent/child/target"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline after one parent creation must stop before the next");

    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(fixture.path().join("new-parent").is_dir());
    assert!(!fixture.path().join("new-parent/child").exists());
}

#[test]
fn test_rooted_copy_deadline_after_destination_revalidation_preserves_target() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"replacement").expect("source should exist");
    fs::create_dir(fixture.path().join("target")).expect("target directory should exist");
    fs::write(fixture.path().join("target/child"), b"original").expect("target child should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::RootedCopyDestinationRevalidated);

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("late deadline must prevent removal of the replacement target");

    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(
        b"original",
        fs::read(fixture.path().join("target/child")).unwrap().as_slice()
    );
}

#[test]
fn test_rooted_copy_deadline_after_writer_identity_check_prevents_install() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"replacement").expect("source should exist");
    fs::write(fixture.path().join("target"), b"original").expect("target should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::RootedWriterIdentityVerified);

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("late deadline must prevent installing staged bytes");

    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(b"original", fs::read(fixture.path().join("target")).unwrap().as_slice());
}

#[test]
fn test_rooted_copy_rejects_unrepresentable_deadline_before_mutation() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"source").expect("source should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("new-parent/target"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::MAX),
        )
        .expect_err("an unrepresentable copy deadline must fail before mutation");

    assert_eq!(LocalFileOperation::Copy, failure.error().operation());
    assert_eq!(LocalFileErrorKind::InvalidOptions, failure.error().kind());
    assert_eq!(Some(Path::new("source")), failure.error().path());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert!(!fixture.path().join("new-parent").exists());
}

#[test]
fn test_rooted_copy_deadline_matrix_preserves_each_parent_creation_boundary() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"payload").expect("source should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let mut expired_attempts = 0;
    let mut completed = false;

    // Sweep each deterministic deadline check up to successful completion. This
    // covers failures during path resolution, parent preflight, and each parent
    // creation while asserting that no target is published on an expired run.
    for allowance in 0..512 {
        let target = PathBuf::from(format!("case-{allowance}")).join("a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/target");
        let _clock = observations::DeadlineClockGuard::expire_after(allowance);
        match filesystem.copy_with_options(
            Path::new("source"),
            &target,
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        ) {
            Ok(outcome) => {
                assert_eq!(1, outcome.stats().files());
                assert_eq!(b"payload", fs::read(fixture.path().join(&target)).unwrap().as_slice());
                completed = true;
                break;
            }
            Err(failure) => {
                assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
                match failure.state() {
                    LocalCopyFailureState::Unchanged | LocalCopyFailureState::PartiallyPublished => {
                        assert!(!fixture.path().join(&target).exists());
                    }
                    LocalCopyFailureState::Published => {
                        assert_eq!(b"payload", fs::read(fixture.path().join(&target)).unwrap().as_slice())
                    }
                    LocalCopyFailureState::Indeterminate => {}
                }
                expired_attempts += 1;
            }
        }
    }

    assert!(
        completed,
        "the deadline sweep must eventually reach successful completion"
    );
    assert!(
        expired_attempts >= 32,
        "expected to cover each resolution and parent boundary"
    );
}

#[test]
fn test_rooted_delete_rejects_expired_and_unrepresentable_deadlines() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(fixture.path().join("target")).expect("target should exist");
    fs::write(fixture.path().join("target/child"), b"preserve").expect("child should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");

    let expired = filesystem
        .delete_directory_with_options(
            Path::new("target"),
            &crate::LocalDeleteOptions::new().with_deadline(Duration::ZERO),
        )
        .expect_err("an expired delete deadline must stop before metadata access");
    assert_eq!(ErrorKind::TimedOut, expired.io_error_kind());

    let overflow = filesystem
        .delete_directory_with_options(
            Path::new("target"),
            &crate::LocalDeleteOptions::new().with_deadline(Duration::MAX),
        )
        .expect_err("an unrepresentable delete deadline must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidPath, overflow.kind());
    assert_eq!(LocalFileOperation::DeleteDirectory, overflow.operation());
    assert_eq!(ErrorKind::InvalidInput, overflow.io_error_kind());
    assert_eq!(Some(Path::new("/target")), overflow.path());
    assert_eq!(
        b"preserve",
        fs::read(fixture.path().join("target/child")).unwrap().as_slice()
    );
}

#[test]
fn test_rooted_delete_deadline_matrix_keeps_directory_until_deletion_can_finish() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let mut expired_attempts = 0;
    let mut completed = false;

    for allowance in 0..32 {
        let target = PathBuf::from(format!("case-{allowance}"));
        let native_target = fixture.path().join(&target);
        fs::create_dir(&native_target).expect("target directory should exist");
        let _clock = observations::DeadlineClockGuard::expire_after(allowance);
        match filesystem.delete_directory_with_options(
            &target,
            &crate::LocalDeleteOptions::new().with_deadline(Duration::from_secs(60)),
        ) {
            Ok(outcome) => {
                assert!(outcome.deleted());
                assert!(!native_target.exists());
                completed = true;
                break;
            }
            Err(error) => {
                assert_eq!(ErrorKind::TimedOut, error.io_error_kind());
                assert!(
                    native_target.is_dir(),
                    "an expired operation must preserve its directory"
                );
                expired_attempts += 1;
            }
        }
    }

    assert!(completed, "the deadline sweep must eventually reach deletion");
    assert!(
        expired_attempts >= 4,
        "expected to cover resolution and metadata boundaries"
    );
}

#[test]
fn test_copy_budget_absolute_deadline_preserves_origin_and_rejects_overflow() {
    let no_deadline = CopyBudget::new(LocalCopyDirOptions::new());
    assert_eq!(
        None,
        no_deadline.absolute_deadline().expect("unbounded deadline is valid")
    );

    let started_at = Instant::now();
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
        ErrorKind::InvalidInput,
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
            Path::new("a/b/c/source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline during source resolution must stop the copy");

    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
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
            Path::new("source"),
            Path::new("new-parent/target"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired resolution must stop before parent creation");

    assert_eq!(LocalFileOperation::Copy, failure.error().operation());
    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
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
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline after replacement removal must report partial publication");

    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(!fixture.path().join("target").exists());
}

#[cfg(windows)]
#[test]
fn test_rooted_link_kind_deadline_keeps_existing_file() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(fixture.path().join("source")).expect("source directory should exist");
    fs::create_dir(fixture.path().join("target")).expect("target directory should exist");
    symlink_file("missing-target", fixture.path().join("source/link")).expect("source symbolic link should be created");
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

fn assert_link_kind_deadline_keeps_existing_file(root: &Path) {
    fs::write(root.join("target/link"), b"protected").expect("preexisting target should exist");
    let filesystem = LocalFileSystem::rooted(root).expect("rooted filesystem should open");
    let _clock = observations::DeadlineClockGuard::expire_at(DeadlineBoundary::RootedLinkKindInspected);

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired deadline after link inspection must stop before target removal");
    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
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
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired link resolution must stop before replacing its destination");

    assert_eq!(ErrorKind::TimedOut, failure.error().io_error_kind());
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
