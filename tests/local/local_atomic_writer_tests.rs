// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Write;
#[cfg(unix)]
use std::io::{
    ErrorKind,
    IoSlice,
};
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::time::Duration;

use super::api_tests::{
    LocalAtomicDestinationState,
    LocalAtomicWriteOptions,
    LocalAtomicWriteStage,
    LocalAtomicWriter,
};

#[cfg(target_os = "linux")]
use super::test_support::SourceReadLease;
use super::test_support::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
    count_atomic_temp_files,
    fs,
    temp_dir,
};

/// Asserts at compile time that `T` implements `Send`.
fn assert_send<T: Send>() {}

#[test]
fn test_local_atomic_writer_is_send() {
    assert_send::<LocalAtomicWriter>();
}

#[test]
fn test_atomic_writer_options_do_not_create_missing_parent_by_default() {
    let dir = temp_dir("atomic-writer-parent-disabled");
    let parent = dir.join("missing").join("nested");
    let path = parent.join("out.txt");

    let error = qubit_local_files::atomic::begin_with(
        &path,
        LocalAtomicWriteOptions::new(),
    )
    .expect_err("missing parent should be rejected");

    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    assert!(!parent.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_writer_options_can_create_missing_parents() {
    let dir = temp_dir("atomic-writer-parent-enabled");
    let parent = dir.join("missing").join("nested");
    let path = parent.join("out.txt");

    let mut writer = qubit_local_files::atomic::begin_with(
        &path,
        LocalAtomicWriteOptions::new().with_parent(),
    )
    .expect("parent-enabled writer should begin");
    writer.write_all(b"payload").expect("payload should stage");
    writer.commit().expect("payload should commit");

    assert_eq!(b"payload", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_local_atomic_writer_zero_open_retry_timeout_reports_timed_out() {
    let dir = temp_dir("atomic-writer-zero-open-retry-timeout");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").expect("destination should be written");
    let lease = SourceReadLease::acquire(&path)
        .expect("destination read lease should be acquired");
    let options = LocalAtomicWriteOptions::new()
        .with_parent()
        .with_open_retry_timeout(Duration::ZERO);
    let mut writer = qubit_local_files::atomic::begin_with(&path, options)
        .expect("atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        sender.send(writer.commit()).expect("result should be sent");
    });

    lease
        .wait_for_break()
        .expect("commit should reach the destination open");
    let first_result = receiver.recv_timeout(Duration::from_millis(250));
    lease
        .release()
        .expect("destination lease should be released");
    worker.join().expect("commit worker should not panic");
    let result = first_result.unwrap_or_else(|_| {
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("commit result should arrive after lease release")
    });
    let error =
        result.expect_err("zero timeout should reject the lease conflict");

    assert_eq!(
        LocalAtomicWriteStage::ReadDestinationMetadata,
        error.stage()
    );
    assert_eq!(ErrorKind::TimedOut, error.kind());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state()
    );
    assert_eq!(b"original", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_commits_written_contents() {
    let dir = temp_dir("atomic-writer-commit");
    let path = dir.join("out.txt");
    let mut writer = qubit_local_files::atomic::begin(&path)
        .expect("atomic writer should begin");
    writer
        .write_all(b"committed")
        .expect("contents should write");
    writer.flush().expect("staging contents should flush");
    assert!(!path.exists());
    writer.commit().expect("atomic writer should commit");
    assert_eq!(b"committed", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_local_atomic_writer_forwards_vectored_writes() {
    let dir = temp_dir("atomic-writer-vectored");
    let path = dir.join("out.txt");
    let mut writer = qubit_local_files::atomic::begin(&path)
        .expect("atomic writer should begin");
    let buffers = [IoSlice::new(b"ab"), IoSlice::new(b"cd")];

    let count = writer
        .write_vectored(&buffers)
        .expect("vectored write should succeed");
    writer.commit().expect("atomic writer should commit");

    assert_eq!(4, count);
    assert_eq!(b"abcd", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_local_atomic_writer_rejects_symlink_installed_before_commit() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-writer-commit-symlink");
    let path = dir.join("out.txt");
    let target = dir.join("target.txt");
    fs::write(&target, b"original target").expect("target should be written");
    let mut writer = qubit_local_files::atomic::begin(&path)
        .expect("atomic writer should begin with an absent destination");
    writer
        .write_all(b"replacement")
        .expect("staged contents should be written");
    assert_eq!(1, count_atomic_temp_files(&dir));
    symlink(&target, &path).expect("destination symlink should be installed");

    let error = writer
        .commit()
        .expect_err("commit should reject the destination symlink");

    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(b"original target", fs::read(&target).unwrap().as_slice());
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_keeps_relative_destination_after_cwd_change() {
    let _lock = CURRENT_DIR_LOCK.lock().unwrap();
    let dir = temp_dir("atomic-writer-relative-cwd");
    let creation_dir = dir.join("creation");
    let later_dir = dir.join("later");
    fs::create_dir_all(&creation_dir).unwrap();
    fs::create_dir_all(&later_dir).unwrap();

    let result = {
        let _guard = CurrentDirGuard::change_to(&creation_dir);
        let mut writer =
            qubit_local_files::atomic::begin(std::path::Path::new("out.txt"))
                .expect("relative atomic writer should begin");
        writer.write_all(b"committed").unwrap();
        std::env::set_current_dir(&later_dir).unwrap();
        writer.commit()
    };
    let creation_contents = fs::read(creation_dir.join("out.txt"));
    let later_destination_exists = later_dir.join("out.txt").exists();
    drop(fs::remove_dir_all(&dir));

    result.expect("commit should remain bound to its creation directory");
    assert_eq!(b"committed", creation_contents.unwrap().as_slice());
    assert!(!later_destination_exists);
}

#[cfg(target_os = "linux")]
#[test]
fn test_local_atomic_writer_reports_abort_cleanup_error() {
    let dir = temp_dir("atomic-writer-abort-error");
    let path = dir.join("out.txt");
    let writer = qubit_local_files::atomic::begin(&path).unwrap();
    let temporary_path = atomic_staging_path(&dir);
    fs::remove_file(&temporary_path).unwrap();

    let error = writer
        .abort()
        .expect_err("missing staging path should reject explicit cleanup");

    assert_eq!(
        qubit_local_files::atomic::Stage::CleanupTemporaryFile,
        error.stage()
    );
    assert_eq!(Some(temporary_path.as_path()), error.temporary_path());
    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_local_atomic_writer_preserves_destination_inspection_context() {
    use std::error::Error as StdError;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = temp_dir("atomic-writer-inspection-error");
    let path = dir.join(OsString::from_vec(b"out\0invalid".to_vec()));

    let error = qubit_local_files::atomic::begin(&path)
        .expect_err("NUL destination should fail inspection");
    let source = StdError::source(&error).expect("native source should exist");
    let native_source = source
        .source()
        .expect("native source should remain in the error chain");

    assert_eq!(
        qubit_local_files::atomic::Stage::InspectDestination,
        error.stage()
    );
    assert!(source.to_string().contains("read destination metadata"));
    assert_eq!(
        std::io::ErrorKind::InvalidInput,
        native_source
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_abort_preserves_destination() {
    let dir = temp_dir("atomic-writer-abort");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").unwrap();
    let mut writer = qubit_local_files::atomic::begin(&path).unwrap();
    writer.write_all(b"replacement").unwrap();
    writer.abort().expect("atomic writer should abort");
    assert_eq!(b"original", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_retains_staging_when_destination_disappears() {
    let dir = temp_dir("atomic-writer-missing-destination");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").expect("destination should be written");
    let mut writer = qubit_local_files::atomic::begin(&path)
        .expect("atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    fs::remove_file(&path).expect("destination should be removed");

    let error = writer
        .commit()
        .expect_err("missing destination should reject replacement");
    let staging_path = error
        .temporary_path()
        .map(ToOwned::to_owned)
        .expect("missing-state error should retain its staging path");

    assert_eq!(
        LocalAtomicWriteStage::ReadDestinationMetadata,
        error.stage(),
    );
    assert_eq!(
        LocalAtomicDestinationState::Missing,
        error.destination_state(),
    );
    assert_eq!(
        b"replacement",
        fs::read(&staging_path)
            .expect("retained staging data should be readable")
            .as_slice(),
    );
    fs::remove_file(staging_path).expect("retained staging file should remove");
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_local_atomic_writer_recoverable_commit_returns_writer_for_abort() {
    let dir = temp_dir("atomic-writer-recoverable-commit");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").expect("destination should be written");
    let mut writer = qubit_local_files::atomic::begin(&path)
        .expect("atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    fs::remove_file(&path).expect("destination should be removed");

    let mut commit_error = writer
        .commit_recoverable()
        .expect_err("missing destination should retain the writer");
    assert_eq!(
        LocalAtomicDestinationState::Missing,
        commit_error.error().destination_state(),
    );
    assert!(commit_error.writer().is_some());
    assert!(commit_error.writer_mut().is_some());
    let (error, writer) = commit_error.into_parts();
    let writer = writer.expect("pre-publication failure should return writer");
    let staging_path = error
        .temporary_path()
        .map(ToOwned::to_owned)
        .expect("missing-state error should retain its staging path");

    assert_eq!(
        LocalAtomicDestinationState::Missing,
        error.destination_state(),
    );
    writer
        .abort()
        .expect("returned writer should explicitly remove staging");
    assert!(
        !staging_path.exists(),
        "explicit abort must remove the recoverable staging file",
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_local_atomic_writer_drop_removes_staging_file() {
    let dir = temp_dir("atomic-writer-drop");
    let path = dir.join("out.txt");
    {
        let mut writer = qubit_local_files::atomic::begin(&path).unwrap();
        writer.write_all(b"discarded").unwrap();
    }
    assert!(!path.exists());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

/// Returns the sole atomic staging path in `dir`.
///
/// # Parameters
///
/// * `dir` - Directory containing one atomic staging entry.
///
/// # Returns
///
/// The discovered staging path.
///
/// # Panics
///
/// Panics when the directory cannot be read, an entry cannot be inspected, or
/// no atomic staging entry exists.
#[cfg(target_os = "linux")]
fn atomic_staging_path(dir: &Path) -> std::path::PathBuf {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name().and_then(|name| name.to_str()).is_some_and(
                |name| {
                    name.starts_with(".atomic-write-") && name.ends_with(".tmp")
                },
            )
        })
        .expect("atomic staging path should exist")
}
