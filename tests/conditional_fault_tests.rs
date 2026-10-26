// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(all(feature = "digest", feature = "test-support"))]

mod conditional {
    use qubit_io::Input;
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::policy::LocalSymlinkPolicy;

    pub fn host() -> LocalFileSystem {
        let mut filesystem = LocalFileSystem::host().expect("host filesystem");
        filesystem
            .set_symlink_policy(LocalSymlinkPolicy::Reject)
            .expect("reject symlinks");
        filesystem
    }

    pub fn read_candidate(error: &qubit_local_files::error::LocalConditionalCommitError) -> Vec<u8> {
        let candidate = error.candidate().expect("verified candidate copy");
        read_recovery(candidate)
    }

    pub fn read_recovery(file: &qubit_local_files::recovery::LocalRecoveryFile) -> Vec<u8> {
        let mut reader = file.open_reader().expect("open recovery file through retained access");
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 128];
        loop {
            match reader.read(&mut buffer).expect("read candidate") {
                0 => return bytes,
                count => bytes.extend_from_slice(&buffer[..count]),
            }
        }
    }
}

use std::error::Error;

use qubit_io::Output;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::test_support::TestConditionalHooks;
use qubit_local_files::test_support::TestConditionalPoint;
use sha2::Digest;
use sha2::Sha256;

#[test]
fn required_file_sync_error_before_publication_is_reported_as_not_published() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeRequiredFileSync,
        || Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "sync file")),
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();
    assert!(!path.exists());
    assert_eq!(
        failure.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(failure.error().source().unwrap().to_string(), "sync file");
    assert_eq!(conditional::read_candidate(&failure), b"B");
}

#[test]
#[cfg(unix)]
fn required_parent_sync_error_is_reported_after_published_file_sync() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeRequiredParentSync,
        || Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "sync parent")),
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();
    let installed = std::fs::read(&path).unwrap_or_else(|error| {
        let names: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        panic!(
            "published target should remain readable ({error}); stage={:?}, publication={:?}, \
             failure={}, directory entries: {names:?}",
            failure.stage(),
            failure.publication(),
            failure,
        );
    });
    assert_eq!(installed, b"B");
    assert_eq!(
        failure.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::Published
    );
    assert_eq!(failure.error().source().unwrap().to_string(), "sync parent");
}

#[cfg(windows)]
#[test]
fn windows_sharing_failure_removes_empty_owned_recovery_container() {
    use std::os::windows::fs::OpenOptionsExt;

    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    std::fs::write(&target, b"A").expect("old target should be written");
    let sharing_lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&target)
        .expect("target should open with delete sharing denied");
    let mut filesystem = conditional::host();
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional writer should open");
    writer.write_fully(b"B").expect("candidate should accept bytes");

    let failure = writer
        .commit()
        .expect_err("sharing lock should prevent ReplaceFileW");

    assert_eq!(std::fs::read(&target).expect("target should remain"), b"A");
    assert_eq!(
        failure.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(conditional::read_candidate(&failure), b"B");
    assert!(
        failure.recovery_files().iter().all(|file| file.role() != LocalRecoveryFileRole::Displaced),
        "no backup should be retained when ReplaceFileW did not create one"
    );
    assert!(
        std::fs::read_dir(directory.path())
            .expect("fixture parent should remain readable")
            .filter_map(Result::ok)
            .all(|entry| !entry.file_type().is_ok_and(|kind| kind.is_dir())),
        "empty recovery container should be removed"
    );
    drop(sharing_lock);
}

#[test]
#[cfg(windows)]
fn required_parent_sync_unavailability_fails_before_publication() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let mut writer = conditional::host()
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();

    let failure = writer.commit().unwrap_err();

    assert!(!path.exists());
    assert_eq!(
        failure.stage(),
        qubit_local_files::error::LocalConditionalWriteStage::Sync
    );
    assert_eq!(
        failure.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert!(
        failure
            .error()
            .to_string()
            .contains("Windows directory durability is not guaranteed"),
        "unexpected Required durability error: {}",
        failure.error()
    );
    assert_eq!(conditional::read_candidate(&failure), b"B");
}

#[test]
fn candidate_copy_verification_fault_does_not_publish() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::AfterCandidateCopy,
        || Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "copy verify")),
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();
    assert!(!path.exists());
    assert!(failure.candidate().is_none());
    let staged = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::StagedCandidate)
        .expect("incomplete candidate is retained as staged");
    assert_eq!(conditional::read_recovery(staged), b"B");
}

#[test]
fn candidate_copy_preparation_failure_keeps_but_does_not_label_candidate_copy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeCandidateCopy,
        || Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "copy create")),
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();
    assert!(!path.exists());
    assert!(failure.candidate().is_none());
    let staged = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::StagedCandidate)
        .expect("staging remains separately identified");
    assert_eq!(conditional::read_recovery(staged), b"B");
}

#[test]
fn cleanup_old_failure_keeps_success_and_displaced_contents() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeCleanupOld,
        || Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "cleanup old")),
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let outcome = writer.commit().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"B");
    assert!(!outcome.cleanup_errors().is_empty());
    assert!(
        outcome
            .retained_files()
            .iter()
            .any(|entry| { entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::Displaced })
    );
}

#[test]
fn cleanup_copy_failure_keeps_success_and_reports_recovery_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeCleanupCopy,
        || {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cleanup copy",
            ))
        },
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let outcome = writer.commit().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"B");
    assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
    assert!(!outcome.cleanup_errors().is_empty());
    assert_eq!(outcome.retained_files().len(), 1);
    assert_eq!(
        outcome.retained_files()[0].role(),
        qubit_local_files::recovery::LocalRecoveryFileRole::CandidateCopy
    );
}

#[test]
fn cleanup_directory_sync_error_keeps_success_even_after_copy_removal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeCleanupSync,
        || {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cleanup sync",
            ))
        },
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let outcome = writer.commit().unwrap();
    assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
    assert_eq!(outcome.retained_files().len(), 0);
    assert!(!outcome.cleanup_errors().is_empty());
}

#[test]
fn panic_after_publication_does_not_remove_candidate_or_unknown_target() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let competitor = path.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::AfterPublish,
        move || {
            std::fs::write(competitor, b"C")?;
            panic!("injected after publication")
        },
    ));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut writer = filesystem
            .open_conditional_writer::<Sha256>(
                &path,
                LocalWritePrecondition::Absent,
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .unwrap();
        writer.write_fully(b"B").unwrap();
        let _ = writer.commit();
    }));
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"C");
}

#[test]
fn panic_before_native_publish_leaves_unpublished_staging_for_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host()
        .with_conditional_test_hooks(TestConditionalHooks::once(TestConditionalPoint::BeforePublish, || {
            panic!("injected before native publish")
        }));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut writer = filesystem
            .open_conditional_writer::<Sha256>(
                &path,
                LocalWritePrecondition::Absent,
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .unwrap();
        writer.write_fully(b"B").unwrap();
        let _ = writer.commit();
    }));
    assert!(result.is_err());
    assert!(!path.exists());
    assert!(std::fs::read_dir(directory.path()).unwrap().next().is_some());
}
