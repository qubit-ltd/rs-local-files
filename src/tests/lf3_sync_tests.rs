// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use qubit_io::Output;

use crate::LocalFileSystem;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalFileSyncRequirement;
use crate::writer::internal::LocalCommitSyncObserver;

#[test]
fn test_per_writer_observer_counts_file_and_parent_syncs_separately() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_durability(LocalDurabilityRequirement::NotRequired);
    let observer = Arc::new(LocalCommitSyncObserver::default());
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open")
        .with_sync_observer(Arc::clone(&observer));
    writer.write_fully(b"synced").expect("bytes should be written");
    let outcome = writer.commit().expect("writer should commit");

    assert!(outcome.file_synced());
    assert!(!outcome.durable());
    assert!(observer.file_sync_calls() > 0);
    assert_eq!(0, observer.parent_sync_calls());
}

#[cfg(feature = "test-support")]
#[test]
fn test_required_sync_failure_preserves_staged_host_targets() {
    for mode in [LocalWriteMode::CreateNew, LocalWriteMode::CreateOrReplace] {
        let fixture = tempfile::tempdir().expect("temporary directory should be created");
        let fs = LocalFileSystem::host().expect("host filesystem should open");
        let target = fixture.path().join("sync");
        if mode == LocalWriteMode::CreateOrReplace {
            std::fs::write(&target, b"original").expect("destination should be created");
        }
        let options = LocalWriteOptions::new(mode)
            .with_file_sync(LocalFileSyncRequirement::Required)
            .with_durability(LocalDurabilityRequirement::NotRequired);
        let mut writer = fs
            .open_writer_with_options(&target, &options)
            .expect("writer should open");
        writer.write_fully(b"replacement").expect("bytes should be staged");
        let _fault =
            crate::test_support::install_test_fault("writer-staged-file-sync").expect("file sync fault should install");
        let failure = writer.commit().expect_err("required sync failure must block install");
        assert_eq!(crate::LocalWriteFailureState::NotPublished, failure.state());
        assert!(failure.writer().is_some());
        match mode {
            LocalWriteMode::CreateNew => assert!(!target.exists()),
            LocalWriteMode::CreateOrReplace => {
                assert_eq!(b"original".to_vec(), std::fs::read(target).unwrap())
            }
            LocalWriteMode::Append => unreachable!(),
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
fn test_required_sync_failure_preserves_staged_rooted_targets() {
    for mode in [LocalWriteMode::CreateNew, LocalWriteMode::CreateOrReplace] {
        let fixture = tempfile::tempdir().expect("temporary directory should be created");
        let fs = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
        let target = std::path::Path::new("sync");
        let host_target = fixture.path().join(target);
        if mode == LocalWriteMode::CreateOrReplace {
            std::fs::write(&host_target, b"original").expect("destination should be created");
        }
        let options = LocalWriteOptions::new(mode)
            .with_file_sync(LocalFileSyncRequirement::Required)
            .with_durability(LocalDurabilityRequirement::NotRequired);
        let mut writer = fs
            .open_writer_with_options(target, &options)
            .expect("writer should open");
        writer.write_fully(b"replacement").expect("bytes should be staged");
        let _fault =
            crate::test_support::install_test_fault("writer-staged-file-sync").expect("file sync fault should install");
        let failure = writer.commit().expect_err("required sync failure must block install");
        assert_eq!(crate::LocalWriteFailureState::NotPublished, failure.state());
        assert!(failure.writer().is_some());
        match mode {
            LocalWriteMode::CreateNew => assert!(!host_target.exists()),
            LocalWriteMode::CreateOrReplace => {
                assert_eq!(b"original".to_vec(), std::fs::read(host_target).unwrap())
            }
            LocalWriteMode::Append => unreachable!(),
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
fn test_preferred_sync_failure_publishes_without_claiming_file_sync() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_file_sync(LocalFileSyncRequirement::Preferred)
        .with_durability(LocalDurabilityRequirement::NotRequired);
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open");
    writer.write_fully(b"data").expect("bytes should be written");
    let _fault =
        crate::test_support::install_test_fault("writer-staged-file-sync").expect("file sync fault should install");
    let outcome = writer.commit().expect("preferred file sync may fail and publish");
    assert!(!outcome.file_synced());
    assert!(!outcome.durable());
    assert_eq!(
        std::fs::read(target).expect("published target should be readable"),
        b"data"
    );
}

#[cfg(feature = "test-support")]
#[test]
fn test_required_sync_failure_retains_staging_before_publish() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_durability(LocalDurabilityRequirement::NotRequired);
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open");
    writer.write_fully(b"data").expect("bytes should be written");
    let _fault =
        crate::test_support::install_test_fault("writer-staged-file-sync").expect("file sync fault should install");
    let failure = writer
        .commit()
        .expect_err("required file sync must fail before publication");
    assert_eq!(crate::LocalWriteFailureState::NotPublished, failure.state());
    assert!(failure.writer().is_some(), "staging should be retained for recovery");
    assert!(!target.exists());
}

#[cfg(feature = "test-support")]
#[test]
fn test_append_sync_failure_preserves_already_written_bytes() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    std::fs::write(&target, b"prefix").expect("append target should be created");
    let options = LocalWriteOptions::new(LocalWriteMode::Append)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_durability(LocalDurabilityRequirement::NotRequired);
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open");
    writer.write_fully(b"data").expect("bytes should be appended");
    let _fault = crate::test_support::install_test_fault("writer-append-required-sync")
        .expect("append sync fault should install");
    let failure = writer
        .commit()
        .expect_err("required append sync should report its failure");
    assert_eq!(crate::LocalWriteFailureState::Published, failure.state());
    assert_eq!(
        b"prefixdata".to_vec(),
        std::fs::read(target).expect("prior append bytes must remain")
    );
}

#[cfg(feature = "test-support")]
#[test]
fn test_append_sync_failure_preserves_rooted_existing_bytes() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let target = std::path::Path::new("sync");
    std::fs::write(fixture.path().join(target), b"prefix").expect("append target should be created");
    let options = LocalWriteOptions::new(LocalWriteMode::Append)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_durability(LocalDurabilityRequirement::NotRequired);
    let mut writer = fs
        .open_writer_with_options(target, &options)
        .expect("writer should open");
    writer.write_fully(b"data").expect("bytes should be appended");
    let _fault = crate::test_support::install_test_fault("writer-append-required-sync")
        .expect("append sync fault should install");
    let failure = writer.commit().expect_err("required append sync should report failure");
    assert_eq!(crate::LocalWriteFailureState::Published, failure.state());
    assert_eq!(
        b"prefixdata".to_vec(),
        std::fs::read(fixture.path().join(target)).unwrap()
    );
}

#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_preferred_parent_sync_failure_preserves_file_sync_result() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("parent/sync");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_create_parent()
        .with_durability(LocalDurabilityRequirement::Preferred);
    let observer = Arc::new(LocalCommitSyncObserver::default());
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open")
        .with_sync_observer(Arc::clone(&observer));
    writer.write_fully(b"data").expect("bytes should be written");
    let _fault = crate::test_support::install_test_fault("atomic-writer-created-parent-sync")
        .expect("parent sync fault should install");
    let outcome = writer.commit().expect("preferred parent sync failure may publish");
    assert!(outcome.file_synced());
    assert!(!outcome.durable());
    assert_eq!(1, observer.file_sync_calls());
    assert_eq!(1, observer.parent_sync_calls());
}

#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_preferred_rooted_parent_sync_failure_preserves_file_sync_result() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let target = std::path::Path::new("sync");
    let options =
        LocalWriteOptions::new(LocalWriteMode::CreateNew).with_durability(LocalDurabilityRequirement::Preferred);
    let observer = Arc::new(LocalCommitSyncObserver::default());
    let mut writer = fs
        .open_writer_with_options(target, &options)
        .expect("writer should open")
        .with_sync_observer(Arc::clone(&observer));
    writer.write_fully(b"data").expect("bytes should be written");
    let _fault = crate::test_support::install_test_fault("rooted-preferred-parent-sync")
        .expect("parent sync fault should install");
    let outcome = writer.commit().expect("preferred parent sync failure may publish");
    assert!(outcome.file_synced());
    assert!(!outcome.durable());
    assert_eq!(1, observer.file_sync_calls());
    assert_eq!(1, observer.parent_sync_calls());
}

#[cfg(all(any(target_os = "linux", target_os = "android"), feature = "test-support"))]
#[test]
fn test_host_recovery_parent_sync_is_counted() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_durability(LocalDurabilityRequirement::Preferred);
    let observer = Arc::new(LocalCommitSyncObserver::default());
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open")
        .with_sync_observer(Arc::clone(&observer));
    writer.write_fully(b"data").expect("bytes should be written");
    let _fault = crate::test_support::install_test_fault("atomic-install-unlink-recover-sync")
        .expect("fallback recovery sync fault should install");
    let outcome = writer.commit().expect("preferred recovery sync may fail and publish");

    assert!(outcome.file_synced());
    assert!(!outcome.durable());
    assert_eq!(1, observer.file_sync_calls());
    assert_eq!(1, observer.parent_sync_calls());
    assert_eq!(
        b"data".to_vec(),
        std::fs::read(target).expect("published file should remain")
    );
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android"), feature = "test-support"))]
#[test]
fn test_rooted_recovery_parent_sync_is_counted() {
    let fixture = tempfile::tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let target = std::path::Path::new("sync");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_durability(LocalDurabilityRequirement::Preferred);
    let observer = Arc::new(LocalCommitSyncObserver::default());
    let mut writer = fs
        .open_writer_with_options(target, &options)
        .expect("writer should open")
        .with_sync_observer(Arc::clone(&observer));
    writer.write_fully(b"data").expect("bytes should be written");
    let _fault = crate::test_support::install_test_fault("atomic-install-unlink-recover-sync")
        .expect("fallback recovery sync fault should install");
    let outcome = writer.commit().expect("preferred recovery sync may fail and publish");

    assert!(outcome.file_synced());
    assert!(!outcome.durable());
    assert_eq!(1, observer.file_sync_calls());
    assert_eq!(1, observer.parent_sync_calls());
    assert_eq!(
        b"data".to_vec(),
        std::fs::read(fixture.path().join(target)).expect("published file should remain")
    );
}
