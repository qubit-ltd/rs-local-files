// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Write;
#[cfg(target_os = "linux")]
use std::path::Path;

use qubit_local_files::{
    LocalAtomicWriter,
    LocalFiles,
};

use super::test_support::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
    count_atomic_temp_files,
    fs,
    temp_dir,
};

fn assert_send<T: Send>() {}

#[test]
fn test_local_atomic_writer_is_send() {
    assert_send::<LocalAtomicWriter>();
}

#[test]
fn test_local_atomic_writer_commits_written_contents() {
    let dir = temp_dir("atomic-writer-commit");
    let path = dir.join("out.txt");
    let mut writer = LocalFiles::begin_atomic_write(&path)
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
        let mut writer = LocalFiles::begin_atomic_write("out.txt")
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
    let writer = LocalFiles::begin_atomic_write(&path).unwrap();
    let temporary_path = atomic_staging_path(&dir);
    fs::remove_file(&temporary_path).unwrap();

    let error = writer
        .abort()
        .expect_err("missing staging path should reject explicit cleanup");

    assert_eq!(
        qubit_local_files::LocalAtomicWriteStage::CleanupTemporaryFile,
        error.stage
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

    let error = LocalFiles::begin_atomic_write(&path)
        .expect_err("NUL destination should fail inspection");
    let source = StdError::source(&error).expect("native source should exist");
    let native_source = source
        .source()
        .expect("native source should remain in the error chain");

    assert_eq!(
        qubit_local_files::LocalAtomicWriteStage::InspectDestination,
        error.stage
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
    let mut writer = LocalFiles::begin_atomic_write(&path).unwrap();
    writer.write_all(b"replacement").unwrap();
    writer.abort().expect("atomic writer should abort");
    assert_eq!(b"original", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_drop_removes_staging_file() {
    let dir = temp_dir("atomic-writer-drop");
    let path = dir.join("out.txt");
    {
        let mut writer = LocalFiles::begin_atomic_write(&path).unwrap();
        writer.write_all(b"discarded").unwrap();
    }
    assert!(!path.exists());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

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
