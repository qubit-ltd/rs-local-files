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
    LocalTempFile,
    Write,
    ensure_test_logger,
    fs,
    temp_dir,
};

#[test]
fn test_temp_file_with_name_uses_system_temp_directory() {
    let file = LocalTempFile::with_name(Some("qubit-local-files-test-"), Some(".tmp"))
        .expect("temp file should be created");
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
fn test_debug_formatting_contains_type_name() {
    let file = LocalTempFile::with_name(Some("qubit-local-files-debug-"), Some(".tmp"))
        .expect("temp file should be created");

    assert!(format!("{file:?}").contains("LocalTempFile"));
}

#[test]
fn test_temp_file_file_and_close_handle() {
    let dir = temp_dir("temp-file-close");
    let mut file = LocalTempFile::in_dir(&dir, Some("close-"), Some(".tmp"), 4)
        .expect("temp file should be created");

    file.file()
        .expect("shared file handle should be available")
        .metadata()
        .expect("metadata should be readable");
    file.close().expect("close should succeed");
    let error = file
        .file()
        .expect_err("closed file handle should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
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
    let mut first_file = LocalTempFile::in_dir(&dir, Some("local-"), Some(".tmp"), 4)
        .expect("first temp file should be created in dir");
    let second_file = LocalTempFile::in_dir(&dir, Some("local-"), Some(".tmp"), 4)
        .expect("second temp file should be created in dir");
    let first_path = first_file.path().to_owned();
    let second_path = second_file.path().to_owned();

    first_file.file_mut().unwrap().write_all(b"abc").unwrap();

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
    let error = LocalTempFile::in_dir(std::env::temp_dir(), None, None, 0)
        .expect_err("zero retries should be invalid");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(
        "temporary entry retry count must be greater than zero",
        error.to_string()
    );
}

#[test]
fn test_temp_file_in_dir_rejects_path_prefix_fragment() {
    let dir = temp_dir("temp-file-create-error");

    let error = LocalTempFile::in_dir(&dir, Some("missing-parent/"), None, 1)
        .expect_err("path-like prefix should be rejected");

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
        let file = LocalTempFile::in_dir(&dir, Some("drop-"), Some(".tmp"), 4)
            .expect("temp file should be created");
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
    let file = LocalTempFile::in_dir(&dir, Some("drop-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    let path = file.path().to_owned();
    fs::remove_file(&path).unwrap();

    drop(file);

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_keep_preserves_file() {
    let dir = temp_dir("temp-file-keep");
    let mut file = LocalTempFile::in_dir(&dir, Some("keep-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    file.file_mut().unwrap().write_all(b"kept").unwrap();

    let path = file.keep();

    assert!(path.exists());
    assert_eq!(b"kept", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_moves_file() {
    let dir = temp_dir("temp-file-persist");
    let mut file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    file.file_mut().unwrap().write_all(b"payload").unwrap();
    let source = file.path().to_owned();
    let target = dir.join("nested").join("result.txt");

    let persisted = file.persist(&target).expect("temp file should persist");

    assert_eq!(target, persisted);
    assert!(!source.exists());
    assert_eq!(b"payload", fs::read(&target).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_cleans_up_when_parent_creation_fails() {
    let dir = temp_dir("temp-file-persist-error");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
        .expect("temp file should be created");
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
