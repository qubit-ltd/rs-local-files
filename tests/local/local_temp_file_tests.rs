/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use super::local_files_tests::{
    ErrorKind,
    FileBuffering,
    FileWriteMode,
    FileWriteOptions,
    LocalPersistOptions,
    LocalTempFile,
    Write,
    ensure_test_logger,
    fs,
    temp_dir,
};

#[test]
fn test_temp_file_with_name_uses_system_temp_directory() {
    let file =
        LocalTempFile::with_name(Some("qubit-local-files-test-"), Some(".tmp")).expect("temp file should be created");
    let name = file
        .path()
        .file_name()
        .expect("temp path should have a file name")
        .to_string_lossy();

    assert!(file.path().starts_with(std::env::temp_dir()));
    assert!(name.starts_with("qubit-local-files-test-"));
    assert!(name.ends_with(".tmp"));
}

#[test]
fn test_temp_file_exists_and_cleanup() {
    let dir = temp_dir("temp-file-cleanup");
    let file = LocalTempFile::in_dir(&dir, Some("cleanup-"), Some(".tmp"), 4).expect("temp file should be created");
    let path = file.path().to_owned();

    assert!(file.exists().expect("temp file existence should be checked"));
    file.cleanup().expect("temp file should be cleaned up");

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_debug_formatting_contains_type_name() {
    let file =
        LocalTempFile::with_name(Some("qubit-local-files-debug-"), Some(".tmp")).expect("temp file should be created");

    assert!(format!("{file:?}").contains("LocalTempFile"));
}

#[test]
fn test_temp_file_metadata_and_close_handle() {
    let dir = temp_dir("temp-file-close");
    let mut file = LocalTempFile::in_dir(&dir, Some("close-"), Some(".tmp"), 4).expect("temp file should be created");

    file.metadata().expect("metadata should be readable");
    file.close().expect("close should succeed");
    let error = file
        .writer(FileWriteOptions::default())
        .expect_err("closed writer should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_writer_flushes_on_close_and_rejects_writes_after_close() {
    let dir = temp_dir("temp-file-writer-close");
    let mut file = LocalTempFile::in_dir(&dir, Some("writer-"), Some(".tmp"), 4).expect("temp file should be created");
    let path = file.path().to_owned();

    {
        let writer = file
            .writer(FileWriteOptions {
                mode: FileWriteMode::CreateOrTruncate,
                buffering: FileBuffering::Buffered { capacity: Some(16) },
                ..FileWriteOptions::default()
            })
            .expect("temp file writer should be configured");
        writer.write_all(b"buffered payload").unwrap();
    }
    file.close().expect("close should flush buffered contents");
    let error = file
        .writer(FileWriteOptions::default())
        .expect_err("closed temp file should reject reopening its writer");

    assert_eq!(b"buffered payload", fs::read(&path).unwrap().as_slice());
    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_writer_reuses_same_options_and_rejects_different_options() {
    let dir = temp_dir("temp-file-writer-options");
    let mut file = LocalTempFile::in_dir(&dir, Some("writer-"), Some(".tmp"), 4).expect("temp file should be created");
    let options = FileWriteOptions::new(FileWriteMode::CreateOrTruncate).buffered_with_capacity(8);

    {
        let writer = file.writer(options).expect("first writer call should configure writer");
        writer.write_all(b"one").unwrap();
    }
    {
        let writer = file.writer(options).expect("same writer options should be accepted");
        writer.write_all(b"-two").unwrap();
    }
    let error = file
        .writer(FileWriteOptions::new(FileWriteMode::AppendExisting))
        .expect_err("different writer options should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    file.close().unwrap();
    assert_eq!(b"one-two", fs::read(file.path()).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_writer_rejects_create_new_because_temp_file_already_exists() {
    let dir = temp_dir("temp-file-writer-create-new");
    let mut file = LocalTempFile::in_dir(&dir, Some("writer-"), Some(".tmp"), 4).expect("temp file should be created");

    let error = file
        .writer(FileWriteOptions {
            mode: FileWriteMode::CreateNew,
            ..FileWriteOptions::default()
        })
        .expect_err("create-new mode should reject an already-created temp file");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_new_creates_unique_existing_files() {
    let first_file = LocalTempFile::new().expect("first temp file should exist");
    let second_file = LocalTempFile::new().expect("second temp file should exist");
    let first_path = first_file.path().to_owned();
    let second_path = second_file.path().to_owned();

    assert_ne!(first_path, second_path);
    assert!(first_path.exists());
    assert!(second_path.exists());
}

#[test]
fn test_temp_file_in_dir_creates_unique_existing_files() {
    let dir = temp_dir("temp-file-in");
    let mut first_file =
        LocalTempFile::in_dir(&dir, Some("local-"), Some(".tmp"), 4).expect("first temp file should be created in dir");
    let second_file = LocalTempFile::in_dir(&dir, Some("local-"), Some(".tmp"), 4)
        .expect("second temp file should be created in dir");
    let first_path = first_file.path().to_owned();
    let second_path = second_file.path().to_owned();

    first_file
        .writer(FileWriteOptions::default())
        .unwrap()
        .write_all(b"abc")
        .unwrap();

    assert_ne!(first_path, second_path);
    assert_eq!(Some(dir.as_path()), first_path.parent());
    assert_eq!(Some(dir.as_path()), second_path.parent());
    assert!(first_path.exists());
    assert!(second_path.exists());

    drop(first_file);
    drop(second_file);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_in_dir_rejects_zero_retry_count() {
    let error = LocalTempFile::in_dir(std::env::temp_dir(), None, None, 0).expect_err("zero retries should be invalid");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(
        "temporary entry retry count must be greater than zero",
        error.to_string()
    );
}

#[test]
fn test_temp_file_in_dir_rejects_path_prefix_fragment() {
    let dir = temp_dir("temp-file-create-error");

    let error =
        LocalTempFile::in_dir(&dir, Some("missing-parent/"), None, 1).expect_err("path-like prefix should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_in_dir_returns_parent_creation_error() {
    let dir = temp_dir("temp-file-parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = LocalTempFile::in_dir(file_parent.join("child"), None, None, 1)
        .expect_err("file parent should return create-dir error");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_drop_removes_file() {
    let dir = temp_dir("temp-file-drop");
    let path = {
        let file = LocalTempFile::in_dir(&dir, Some("drop-"), Some(".tmp"), 4).expect("temp file should be created");
        let path = file.path().to_owned();
        assert!(path.exists());
        path
    };

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_drop_logs_and_ignores_missing_file() {
    ensure_test_logger();
    let dir = temp_dir("temp-file-drop-missing");
    let file = LocalTempFile::in_dir(&dir, Some("drop-"), Some(".tmp"), 4).expect("temp file should be created");
    let path = file.path().to_owned();
    fs::remove_file(&path).unwrap();

    drop(file);

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_keep_preserves_file() {
    let dir = temp_dir("temp-file-keep");
    let mut file = LocalTempFile::in_dir(&dir, Some("keep-"), Some(".tmp"), 4).expect("temp file should be created");
    file.writer(FileWriteOptions::default())
        .unwrap()
        .write_all(b"kept")
        .unwrap();

    let path = file.keep().expect("temp file should be kept");

    assert!(path.exists());
    assert_eq!(b"kept", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_moves_file() {
    let dir = temp_dir("temp-file-persist");
    let mut file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4).expect("temp file should be created");
    file.writer(FileWriteOptions::default())
        .unwrap()
        .write_all(b"payload")
        .unwrap();
    let source = file.path().to_owned();
    let target = dir.join("nested").join("result.txt");

    let persisted = file.persist(&target).expect("temp file should persist");

    assert_eq!(target, persisted);
    assert!(!source.exists());
    assert_eq!(b"payload", fs::read(&target).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_rejects_existing_target_by_default() {
    let dir = temp_dir("temp-file-persist-existing-target");
    let mut file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4).expect("temp file should be created");
    file.writer(FileWriteOptions::default())
        .unwrap()
        .write_all(b"new")
        .unwrap();
    let source = file.path().to_owned();
    let target = dir.join("result.txt");
    fs::write(&target, b"old").unwrap();

    let error = file
        .persist(&target)
        .expect_err("existing target should be rejected by default");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert!(!source.exists());
    assert_eq!(b"old", fs::read(&target).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_with_overwrite_replaces_existing_target() {
    let dir = temp_dir("temp-file-persist-overwrite");
    let mut file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4).expect("temp file should be created");
    file.writer(FileWriteOptions::default())
        .unwrap()
        .write_all(b"new")
        .unwrap();
    let source = file.path().to_owned();
    let target = dir.join("result.txt");
    fs::write(&target, b"old").unwrap();

    let persisted = file
        .persist_with(&target, LocalPersistOptions { overwrite: true })
        .expect("overwrite option should replace existing target");

    assert_eq!(target, persisted);
    assert!(!source.exists());
    assert_eq!(b"new", fs::read(&target).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_with_default_rejects_existing_target() {
    let dir = temp_dir("temp-file-persist-default-existing-target");
    let mut file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4).expect("temp file should be created");
    file.writer(FileWriteOptions::default())
        .unwrap()
        .write_all(b"new")
        .unwrap();
    let source = file.path().to_owned();
    let target = dir.join("result.txt");
    fs::write(&target, b"old").unwrap();

    let error = file
        .persist_with(&target, LocalPersistOptions::default())
        .expect_err("default persist options should reject existing targets");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert!(!source.exists());
    assert_eq!(b"old", fs::read(&target).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_file_persist_rejects_target_with_nul_byte() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = temp_dir("temp-file-persist-nul-target");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4).expect("temp file should be created");
    let source = file.path().to_owned();
    let target = dir.join(OsString::from_vec(b"bad\0target.txt".to_vec()));

    let error = file.persist(&target).expect_err("NUL target should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_file_persist_returns_target_metadata_error() {
    let dir = temp_dir("temp-file-persist-metadata-error");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4).expect("temp file should be created");
    let source = file.path().to_owned();
    let target = dir.join("x".repeat(10_000));

    let error = file
        .persist(&target)
        .expect_err("target metadata error should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_cleans_up_when_parent_creation_fails() {
    let dir = temp_dir("temp-file-persist-error");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4).expect("temp file should be created");
    let source = file.path().to_owned();
    let blocker = dir.join("blocker");
    fs::write(&blocker, b"not a directory").unwrap();

    let error = file
        .persist(blocker.join("target"))
        .expect_err("invalid parent should be returned");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}
