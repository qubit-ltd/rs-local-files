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

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::outcome::LocalWriterState;
use qubit_local_files::test_support::TestConditionalHooks;
use qubit_local_files::test_support::TestConditionalPoint;
use sha2::Digest;
use sha2::Sha256;

#[cfg(unix)]
#[test]
fn rollback_backend_precheck_observes_competitor_without_exchange() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let parent = directory.path().to_path_buf();
    let installed_path = path.clone();
    let rollback_competitor = path.clone();
    let filesystem = conditional::host()
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeVerifyDisplaced,
            move || {
                for entry in std::fs::read_dir(parent)? {
                    let entry = entry?;
                    if entry.path() != installed_path && std::fs::read(entry.path()).ok().as_deref() == Some(b"A") {
                        std::fs::write(entry.path(), b"C")?;
                        return Ok(());
                    }
                }
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "displaced A not found",
                ))
            },
        ))
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::AfterRollbackPrecheck,
            move || std::fs::write(rollback_competitor, b"D"),
        ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();
    assert_eq!(
        failure.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::Indeterminate
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"D");
    assert_eq!(
        failure
            .recovery_files()
            .iter()
            .filter(|entry| { entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::RollbackDisplaced })
            .count(),
        1
    );
    let displaced = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::RollbackDisplaced)
        .unwrap();
    assert_eq!(conditional::read_recovery(displaced), b"C");
    assert_eq!(conditional::read_candidate(&failure), b"B");
}

#[cfg(unix)]
#[test]
fn rollback_exchange_displaces_competitor_and_retains_each_resource_once() {
    use qubit_local_files::outcome::LocalConditionalPublicationState;
    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let parent = directory.path().to_path_buf();
    let installed_path = path.clone();
    let rollback_competitor = path.clone();
    let filesystem = conditional::host()
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeVerifyDisplaced,
            move || {
                for entry in std::fs::read_dir(parent)? {
                    let entry = entry?;
                    if entry.path() != installed_path && std::fs::read(entry.path()).ok().as_deref() == Some(b"A") {
                        std::fs::write(entry.path(), b"C")?;
                        return Ok(());
                    }
                }
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "displaced A not found",
                ))
            },
        ))
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeRollbackExchange,
            move || std::fs::write(rollback_competitor, b"D"),
        ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();

    assert_eq!(failure.publication(), LocalConditionalPublicationState::Indeterminate);
    assert_eq!(std::fs::read(&path).unwrap(), b"C");
    assert_eq!(conditional::read_candidate(&failure), b"B");
    assert_eq!(
        failure
            .recovery_files()
            .iter()
            .filter(|entry| entry.role() == LocalRecoveryFileRole::RollbackDisplaced)
            .count(),
        1
    );
    assert_eq!(
        failure
            .recovery_files()
            .iter()
            .filter(|entry| entry.role() == LocalRecoveryFileRole::Displaced)
            .count(),
        1
    );
    assert_eq!(
        failure
            .recovery_files()
            .iter()
            .filter(|entry| entry.role() == LocalRecoveryFileRole::CandidateCopy)
            .count(),
        1
    );
    let rollback_displaced = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::RollbackDisplaced)
        .unwrap();
    let displaced = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::Displaced)
        .unwrap();
    let candidate = failure.candidate().unwrap();
    assert_eq!(conditional::read_recovery(rollback_displaced), b"D");
    assert_eq!(conditional::read_recovery(displaced), b"C");
    assert_eq!(conditional::read_recovery(candidate), b"B");
    assert_eq!(displaced.namespace_path(), path);
    assert_ne!(candidate.namespace_path(), displaced.namespace_path());
    assert_ne!(candidate.namespace_path(), rollback_displaced.namespace_path());
    assert_ne!(displaced.namespace_path(), rollback_displaced.namespace_path());
}

#[cfg(unix)]
#[test]
fn after_rollback_replacement_is_retained_with_the_observed_identity() {
    use std::os::unix::fs::MetadataExt;

    use qubit_local_files::outcome::LocalConditionalPublicationState;
    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let parent = directory.path().to_path_buf();
    let installed_path = path.clone();
    let replaced_target = path.clone();
    let replacement = directory.path().join("replacement-D");
    let filesystem = conditional::host()
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeVerifyDisplaced,
            move || {
                for entry in std::fs::read_dir(parent)? {
                    let entry = entry?;
                    if entry.path() != installed_path && std::fs::read(entry.path()).ok().as_deref() == Some(b"A") {
                        std::fs::write(entry.path(), b"C")?;
                        return Ok(());
                    }
                }
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "displaced A not found",
                ))
            },
        ))
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::AfterRollback,
            move || {
                std::fs::write(&replacement, b"D")?;
                std::fs::rename(replacement, replaced_target)
            },
        ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();

    assert_eq!(failure.publication(), LocalConditionalPublicationState::Indeterminate);
    assert_eq!(std::fs::read(&path).unwrap(), b"D");
    let displaced = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::Displaced)
        .expect("D target receipt");
    assert_eq!(displaced.namespace_path(), path);
    assert_eq!(conditional::read_recovery(displaced), b"D");
    let reader = displaced.open_reader().unwrap();
    let receipt_metadata = reader.as_file().metadata().unwrap();
    let target_metadata = std::fs::metadata(&path).unwrap();
    assert_eq!(receipt_metadata.dev(), target_metadata.dev());
    assert_eq!(receipt_metadata.ino(), target_metadata.ino());
    assert_eq!(conditional::read_candidate(&failure), b"B");
}

#[test]
fn uncommitted_drop_does_not_publish_candidate() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem: LocalFileSystem = conditional::host();
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    drop(writer);
    assert!(!path.exists());
}

#[test]
fn abort_cleans_unpublished_candidate_without_touching_target() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let mut writer = conditional::host()
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let aborted = writer.abort().unwrap();
    assert_eq!(aborted.bytes_written(), 1);
    assert_eq!(std::fs::read(path).unwrap(), b"A");
}

#[cfg(unix)]
#[test]
fn abort_failure_can_be_retried_after_candidate_name_is_restored() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let mut writer = conditional::host()
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let staging = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|entry| std::fs::read(entry).ok().as_deref() == Some(b"B"))
        .expect("candidate staging entry");
    let moved = directory.path().join("temporarily-moved");
    std::fs::rename(&staging, &moved).unwrap();
    assert!(writer.abort().is_err());
    assert!(writer.write_fully(b"C").is_err());
    std::fs::rename(moved, &staging).unwrap();
    let aborted = writer.abort().unwrap();
    assert_eq!(aborted.state(), LocalWriterState::Aborted);
    assert!(!path.exists());
    assert!(writer.abort().is_err());
}

#[cfg(unix)]
#[test]
fn dropping_commit_error_keeps_recovery_candidate_on_disk() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let competitor = path.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::AfterPublish,
        move || std::fs::write(competitor, b"C"),
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let failure = writer.commit().unwrap_err();
    let candidate_path = failure.candidate().unwrap().namespace_path().to_path_buf();
    drop(failure);
    assert_eq!(std::fs::read(candidate_path).unwrap(), b"B");
    assert_eq!(std::fs::read(path).unwrap(), b"C");
}

#[test]
fn dropping_success_outcome_does_not_remove_retained_cleanup_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeCleanupCopy,
        || Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "retain copy")),
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
    let retained = outcome.retained_files()[0].namespace_path().to_path_buf();
    drop(outcome);
    assert_eq!(std::fs::read(retained).unwrap(), b"B");
}
