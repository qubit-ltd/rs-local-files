// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(all(feature = "digest", feature = "test-support"))]

mod support;

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
#[cfg(unix)]
use qubit_local_files::outcome::LocalWriterState;
use qubit_local_files::test_support::TestConditionalHooks;
use qubit_local_files::test_support::TestConditionalPoint;
use sha2::Digest;
use sha2::Sha256;

#[cfg(windows)]
fn windows_file_identity(path: &std::path::Path) -> (u32, u64) {
    use std::fs::File;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;

    let file = File::open(path).expect("identity path should open");
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live handle and `information` is the correctly sized
    // output buffer.
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) };
    assert_ne!(result, 0, "Windows should report the native file identity");

    let file_index = (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    (information.dwVolumeSerialNumber, file_index)
}

#[cfg(windows)]
fn find_windows_backup_file(parent: &std::path::Path) -> std::io::Result<(std::path::PathBuf, std::path::PathBuf)> {
    fn visit(directory: &std::path::Path) -> std::io::Result<Option<(std::path::PathBuf, std::path::PathBuf)>> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if entry.file_name().to_string_lossy().starts_with(".qubit-conditional-recovery-") {
                    for child in std::fs::read_dir(&path)? {
                        let child = child?;
                        if child.file_type()?.is_file() && std::fs::read(child.path())?.as_slice() == b"A" {
                            return Ok(Some((path, child.path())));
                        }
                    }
                }
                if let Some(found) = visit(&path)? {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    visit(parent)?.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Windows backup container with A was not found")
    })
}

#[cfg(windows)]
#[test]
fn windows_cleanup_removed_file_reports_container_residual() {
    use std::sync::Arc;
    use std::sync::Mutex;

    let directory = support::conditional_temp_directory::tempdir().expect("fixture directory");
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").expect("old target should be written");
    let backup = Arc::new(Mutex::new(None));
    let hook_backup = Arc::clone(&backup);
    let parent = directory.path().to_path_buf();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeCleanupOld,
        move || {
            let (backup_container, backup_file) = find_windows_backup_file(&parent)?;
            std::fs::write(backup_container.join("marker"), b"leave this entry").map(|()| {
                *hook_backup.lock().expect("backup lock") = Some((backup_container, backup_file));
            })
        },
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional writer should open");
    writer.write_fully(b"B").expect("candidate should be written");
    let outcome = writer.commit().expect("publication should succeed");

    let (container, backup_file) = backup.lock().expect("backup lock").clone().expect("backup container");
    assert_eq!(std::fs::read(&path).expect("installed target"), b"B");
    assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
    assert!(outcome.cleanup_errors().iter().any(|error| error.path() == Some(container.as_path())));
    assert!(outcome.retained_files().iter().all(|file| file.namespace_path() != backup_file.as_path()));
    assert_eq!(std::fs::read(container.join("marker")).expect("marker remains"), b"leave this entry");
}

#[cfg(windows)]
#[test]
fn windows_cleanup_failed_file_remains_readable() {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::Arc;
    use std::sync::Mutex;

    use qubit_local_files::recovery::LocalRecoveryFileRole;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;

    let directory = support::conditional_temp_directory::tempdir().expect("fixture directory");
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").expect("old target should be written");
    let held_file = Arc::new(Mutex::new(None));
    let hook_file = Arc::clone(&held_file);
    let parent = directory.path().to_path_buf();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeCleanupOld,
        move || {
            let (_, backup_file) = find_windows_backup_file(&parent)?;
            let file = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .open(backup_file)?;
            *hook_file.lock().expect("file lock") = Some(file);
            Ok(())
        },
    ));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional writer should open");
    writer.write_fully(b"B").expect("candidate should be written");
    let outcome = writer.commit().expect("publication should succeed");
    drop(
        held_file
            .lock()
            .expect("file lock")
            .take()
            .expect("hook should hold the backup without DELETE sharing"),
    );

    let retained = outcome
        .retained_files()
        .iter()
        .find(|file| file.role() == LocalRecoveryFileRole::Displaced)
        .expect("failed file deletion should retain old file");
    assert!(outcome.cleanup_errors().iter().any(|error| error.path() == Some(retained.namespace_path())));
    assert_eq!(conditional::read_recovery(retained), b"A");
}

#[cfg(windows)]
fn mutate_displaced_copy(parent: &std::path::Path, installed: &std::path::Path) -> std::io::Result<()> {
    fn visit(directory: &std::path::Path, installed: &std::path::Path) -> std::io::Result<bool> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path == installed {
                continue;
            }
            if entry.file_type()?.is_dir() {
                if visit(&path, installed)? {
                    return Ok(true);
                }
            } else if entry.file_type()?.is_file() && std::fs::read(&path).ok().as_deref() == Some(b"A") {
                std::fs::write(path, b"C")?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    if visit(parent, installed)? {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "displaced A not found",
        ))
    }
}

#[cfg(unix)]
#[test]
fn rollback_backend_precheck_observes_competitor_without_exchange() {
    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
            .filter(|entry| { entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::Displaced })
            .count(),
        1
    );
    let displaced = failure
        .recovery_files()
        .iter()
        .find(|entry| entry.role() == qubit_local_files::recovery::LocalRecoveryFileRole::Displaced)
        .unwrap();
    assert_eq!(conditional::read_recovery(displaced), b"C");
    assert_eq!(conditional::read_candidate(&failure), b"B");
}

#[cfg(unix)]
#[test]
fn rollback_exchange_displaces_competitor_and_retains_each_resource_once() {
    use qubit_local_files::outcome::LocalConditionalPublicationState;
    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
    assert_eq!(displaced.diagnostic_path(), Some(path.as_path()));
    assert_eq!(candidate.diagnostic_path(), Some(path.as_path()));
    assert_ne!(candidate.namespace_path(), displaced.namespace_path());
    assert_ne!(candidate.namespace_path(), rollback_displaced.namespace_path());
    assert_ne!(displaced.namespace_path(), rollback_displaced.namespace_path());
}

#[cfg(windows)]
#[test]
fn windows_rollback_exchange_receipts_track_restored_and_competing_identities() {
    use qubit_local_files::outcome::LocalConditionalPublicationState;
    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = support::conditional_temp_directory::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let original_identity = windows_file_identity(&path);
    let parent = directory.path().to_path_buf();
    let installed_path = path.clone();
    let replacement_path = path.clone();
    let competitor_identity = std::sync::Arc::new(std::sync::Mutex::new(None));
    let observed_competitor_identity = std::sync::Arc::clone(&competitor_identity);
    let filesystem = conditional::host()
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeVerifyDisplaced,
            move || mutate_displaced_copy(&parent, &installed_path),
        ))
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeRollbackExchange,
            move || {
                std::fs::remove_file(&replacement_path)?;
                std::fs::write(&replacement_path, b"C")?;
                *observed_competitor_identity
                    .lock()
                    .expect("competitor identity lock should be available") =
                    Some(windows_file_identity(&replacement_path));
                Ok(())
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

    assert_eq!(
        failure.publication(),
        LocalConditionalPublicationState::Restored,
        "{failure} (reason: {:?})",
        failure.reason()
    );
    let receipts = failure.recovery_files();
    let restored = receipts
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::Displaced && entry.namespace_path() == path)
        .expect("restored target should have a Displaced receipt");
    let competitor = receipts
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::RollbackDisplaced)
        .expect("target competitor should have a RollbackDisplaced receipt");
    let candidate = receipts
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::CandidateCopy)
        .expect("published B should have a CandidateCopy receipt");
    assert_eq!(receipts.len(), 3, "every native object should have one receipt");

    assert_eq!(std::fs::read(&path).unwrap(), b"C");
    assert_eq!(conditional::read_recovery(restored), b"C");
    assert_eq!(conditional::read_recovery(competitor), b"C");
    assert_eq!(conditional::read_recovery(candidate), b"B");
    assert_eq!(restored.namespace_path(), path);
    assert_eq!(restored.diagnostic_path(), Some(path.as_path()));
    assert_ne!(competitor.namespace_path(), path);
    assert_eq!(windows_file_identity(restored.namespace_path()), original_identity);
    let target_identity = windows_file_identity(path.as_path());
    let competitor_file_identity = windows_file_identity(competitor.namespace_path());
    let candidate_copy_identity = windows_file_identity(candidate.namespace_path());
    assert_eq!(
        competitor_file_identity,
        competitor_identity
            .lock()
            .expect("competitor identity lock should be available")
            .expect("competitor identity should have been recorded")
    );
    assert_ne!(target_identity, competitor_file_identity);
    assert_ne!(target_identity, candidate_copy_identity);
    assert_ne!(competitor_file_identity, candidate_copy_identity);
}

#[cfg(windows)]
#[test]
fn windows_failed_rollback_exchange_preserves_published_candidate_and_displaced_file() {
    use qubit_local_files::error::LocalConditionalWriteStage;
    use qubit_local_files::outcome::LocalConditionalPublicationState;
    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = support::conditional_temp_directory::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let original_identity = windows_file_identity(&path);
    let parent = directory.path().to_path_buf();
    let installed_path = path.clone();
    let filesystem = conditional::host()
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeVerifyDisplaced,
            move || mutate_displaced_copy(&parent, &installed_path),
        ))
        .with_conditional_test_hooks(TestConditionalHooks::once(
            TestConditionalPoint::BeforeRollbackExchange,
            || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "rollback blocked",
                ))
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

    assert_eq!(failure.stage(), LocalConditionalWriteStage::Rollback);
    assert_eq!(failure.publication(), LocalConditionalPublicationState::Published);
    assert_eq!(std::fs::read(&path).unwrap(), b"B");
    assert_eq!(conditional::read_candidate(&failure), b"B");
    let receipts = failure.recovery_files();
    assert_eq!(
        receipts.len(),
        3,
        "installed B, displaced C and candidate copy B remain"
    );
    let displaced = receipts
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::Displaced && entry.namespace_path() != path)
        .expect("original displaced file should remain recoverable");
    let installed = receipts
        .iter()
        .find(|entry| entry.namespace_path() == path)
        .expect("still-published candidate should have a receipt");
    let candidate = failure.candidate().unwrap();
    assert_eq!(conditional::read_recovery(displaced), b"C");
    assert_eq!(conditional::read_recovery(installed), b"B");
    assert_eq!(windows_file_identity(displaced.namespace_path()), original_identity);
    assert_ne!(windows_file_identity(&path), original_identity);
    assert_ne!(
        windows_file_identity(candidate.namespace_path()),
        windows_file_identity(&path)
    );
    assert!(
        receipts
            .iter()
            .all(|entry| entry.role() != LocalRecoveryFileRole::RollbackDisplaced)
    );
}

#[cfg(windows)]
#[test]
fn windows_rollback_without_target_replacement_restores_original_identity() {
    use qubit_local_files::outcome::LocalConditionalPublicationState;
    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = support::conditional_temp_directory::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, b"A").unwrap();
    let original_identity = windows_file_identity(&path);
    let parent = directory.path().to_path_buf();
    let installed_path = path.clone();
    let filesystem = conditional::host().with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeVerifyDisplaced,
        move || mutate_displaced_copy(&parent, &installed_path),
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
        LocalConditionalPublicationState::Restored,
        "{failure} (reason: {:?})",
        failure.reason()
    );
    let receipts = failure.recovery_files();
    assert_eq!(receipts.len(), 3, "every native object should have one receipt");
    let restored = receipts
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::Displaced && entry.namespace_path() == path)
        .expect("restored target should have a Displaced receipt");
    let rollback_displaced = receipts
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::RollbackDisplaced)
        .expect("published B should have a RollbackDisplaced receipt");
    let candidate = receipts
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::CandidateCopy)
        .expect("candidate copy B should have a CandidateCopy receipt");

    assert_eq!(std::fs::read(&path).unwrap(), b"C");
    assert_eq!(conditional::read_recovery(restored), b"C");
    assert_eq!(conditional::read_recovery(rollback_displaced), b"B");
    assert_eq!(conditional::read_recovery(candidate), b"B");
    assert_eq!(restored.namespace_path(), path);
    assert_eq!(restored.diagnostic_path(), Some(path.as_path()));
    assert_ne!(rollback_displaced.namespace_path(), path);
    assert_ne!(candidate.namespace_path(), path);
    assert_ne!(rollback_displaced.namespace_path(), candidate.namespace_path());

    let target_identity = windows_file_identity(path.as_path());
    let rollback_displaced_identity = windows_file_identity(rollback_displaced.namespace_path());
    let candidate_copy_identity = windows_file_identity(candidate.namespace_path());
    assert_eq!(target_identity, original_identity);
    assert_ne!(target_identity, rollback_displaced_identity);
    assert_ne!(target_identity, candidate_copy_identity);
    assert_ne!(rollback_displaced_identity, candidate_copy_identity);
}

#[cfg(unix)]
#[test]
fn after_rollback_replacement_is_retained_with_the_observed_identity() {
    use std::os::unix::fs::MetadataExt;

    use qubit_local_files::outcome::LocalConditionalPublicationState;
    use qubit_local_files::recovery::LocalRecoveryFileRole;

    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
    let directory = support::conditional_temp_directory::tempdir().unwrap();
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
