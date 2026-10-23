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
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::outcome::LocalConditionalPublicationState;
use qubit_local_files::test_support::TestConditionalHooks;
use qubit_local_files::test_support::TestConditionalPoint;
use qubit_local_files::test_support::TestFaultPlan;
use qubit_local_files::test_support::TestFaultPoint;
use sha2::Digest;
use sha2::Sha256;

#[test]
fn absent_race_preserves_late_file_and_independent_candidate_copy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let competitor = path.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforePublish,
        move || std::fs::write(competitor, b"C"),
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
    assert_eq!(std::fs::read(&path).unwrap(), b"C");
    assert_eq!(conditional::read_candidate(&failure), b"B");
    assert_eq!(failure.publication(), LocalConditionalPublicationState::NotPublished);
}

#[cfg(unix)]
#[test]
fn after_precheck_inode_content_change_is_detected_and_restored() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let competing = path.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::AfterPrecheck,
        move || std::fs::write(competing, b"C"),
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
    assert_eq!(std::fs::read(&path).unwrap(), b"C");
    assert_eq!(failure.publication(), LocalConditionalPublicationState::Restored);
    assert_eq!(conditional::read_candidate(&failure), b"B");
}

#[cfg(unix)]
#[test]
fn same_contents_on_a_new_inode_remain_eligible_for_content_publish() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let replacement = directory.path().join("replacement");
    std::fs::write(&path, b"A").unwrap();
    let observed = path.clone();
    let replacement_in_hook = replacement.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::AfterPrecheck,
        move || {
            std::fs::rename(&observed, &replacement_in_hook)?;
            std::fs::write(observed, b"A")
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
    let outcome = writer.commit().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"B");
    assert_eq!(std::fs::read(replacement).unwrap(), b"A");
    assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
}

#[cfg(unix)]
#[test]
fn aba_back_to_expected_contents_is_allowed_by_content_condition() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let raced_path = path.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::AfterPrecheck,
        move || {
            std::fs::write(&raced_path, b"C")?;
            std::fs::write(&raced_path, b"A")
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
    let outcome = writer.commit().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"B");
    assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
}

#[cfg(unix)]
#[test]
fn symlink_replacement_after_precheck_is_not_followed_or_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    let external = directory.path().join("external");
    let old = directory.path().join("old");
    std::fs::write(&path, b"A").unwrap();
    std::fs::write(&external, b"outside").unwrap();
    let target = path.clone();
    let old_path = old.clone();
    let external_target = external.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::AfterPrecheck,
        move || {
            std::fs::rename(&target, old_path)?;
            std::os::unix::fs::symlink(external_target, target)
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
    assert_eq!(std::fs::read(&path).unwrap(), b"B");
    assert_eq!(std::fs::read(external).unwrap(), b"outside");
    assert_eq!(std::fs::read(old).unwrap(), b"A");
    assert_eq!(failure.publication(), LocalConditionalPublicationState::Indeterminate);
    let displaced = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::Displaced)
        .expect("symlink displaced entry retained");
    assert!(
        std::fs::symlink_metadata(displaced.namespace_path())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(conditional::read_candidate(&failure), b"B");
}

#[cfg(unix)]
#[test]
fn after_publish_change_is_never_overwritten_during_recovery() {
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
    assert_eq!(std::fs::read(&path).unwrap(), b"C");
    assert_eq!(conditional::read_candidate(&failure), b"B");
    assert!(
        failure
            .recovery_files()
            .iter()
            .any(|entry| { entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::Displaced })
    );
    drop(failure);
    assert_eq!(std::fs::read(&path).unwrap(), b"C");
}

#[test]
fn hooks_are_instance_local_and_shared_by_clones() {
    let directory = tempfile::tempdir().unwrap();
    let first_path = directory.path().join("first");
    let second_path = directory.path().join("second");
    let first_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = first_flag.clone();
    let configured = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforePublish,
        move || {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        },
    ));
    let clone = configured.clone();
    for (filesystem, path) in [(&configured, &first_path), (&clone, &second_path)] {
        let mut writer = filesystem
            .open_conditional_writer::<Sha256>(
                path,
                LocalWritePrecondition::Absent,
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .unwrap();
        writer.write_fully(b"B").unwrap();
        let _outcome = writer.commit().unwrap();
    }
    assert!(first_flag.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(std::fs::read(first_path).unwrap(), b"B");
    assert_eq!(std::fs::read(second_path).unwrap(), b"B");
}

#[test]
fn another_instance_does_not_consume_configured_hook() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = flag.clone();
    let configured = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforePublish,
        move || {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        },
    ));
    let unrelated = conditional::host();
    let mut writer = unrelated
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let _outcome = writer.commit().unwrap();
    assert!(!flag.load(std::sync::atomic::Ordering::SeqCst));
    drop(configured);
}

#[test]
fn conditional_hooks_and_existing_test_faults_survive_core_reconfiguration() {
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("existing");
    let target = directory.path().join("target");
    std::fs::write(&existing, b"A").unwrap();
    let hook_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = hook_ran.clone();
    let filesystem = conditional::host()
        .with_test_faults(Some(TestFaultPlan::fail_once(
            TestFaultPoint::Metadata,
            std::io::ErrorKind::PermissionDenied,
        )))
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforePublish,
            move || {
                observed.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        ));
    assert_eq!(
        filesystem.metadata(&existing).unwrap_err().operation(),
        LocalFileOperation::Metadata
    );
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let _outcome = writer.commit().unwrap();
    assert!(hook_ran.load(std::sync::atomic::Ordering::SeqCst));
}
