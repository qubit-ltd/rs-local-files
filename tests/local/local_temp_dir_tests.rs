/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

#[cfg(unix)]
use super::local_files_tests::PermissionsExt;
use super::local_files_tests::{
    ErrorKind,
    LocalTempDir,
    ensure_test_logger,
    fs,
    temp_dir,
};

#[test]
fn test_debug_formatting_contains_type_name() {
    let dir = LocalTempDir::with_prefix(Some("qubit-local-files-debug-"))
        .expect("temp directory should be created");

    assert!(format!("{dir:?}").contains("LocalTempDir"));
}

#[test]
fn test_temp_dir_with_prefix_creates_existing_directory() {
    let dir = LocalTempDir::with_prefix(Some("qubit-local-files-dir-"))
        .expect("temp directory should be created");
    let name = dir
        .path()
        .file_name()
        .expect("temp directory should have a name")
        .to_string_lossy();

    assert!(dir.path().starts_with(std::env::temp_dir()));
    assert!(dir.path().is_dir());
    assert!(name.starts_with("qubit-local-files-dir-"));
}

#[test]
fn test_temp_dir_new_and_keep_preserves_directory() {
    let dir = LocalTempDir::new().expect("temp directory should be created");
    let path = dir.keep();

    assert!(path.is_dir());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn test_temp_dir_in_dir_rejects_path_prefix_fragment() {
    let dir = temp_dir("temp-dir-create-error");

    let error = LocalTempDir::in_dir(&dir, Some("missing-parent/"), 1)
        .expect_err("path-like prefix should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_in_dir_returns_parent_creation_error() {
    let dir = temp_dir("temp-dir-parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = LocalTempDir::in_dir(file_parent.join("child"), None, 1)
        .expect_err("file parent should return create-dir error");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_in_dir_rejects_zero_retry_count() {
    let dir = temp_dir("temp-dir-zero-retries");

    let error = LocalTempDir::in_dir(&dir, None, 0).expect_err("zero retries should be invalid");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_in_dir_returns_create_error() {
    let dir = temp_dir("temp-dir-permission-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();

    let error = LocalTempDir::in_dir(&dir, Some("local-"), 1)
        .expect_err("unwritable directory should return create-dir error");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_drop_removes_directory_tree() {
    let dir = temp_dir("temp-dir-drop");
    let path = {
        let temp_dir =
            LocalTempDir::in_dir(&dir, Some("drop-"), 4).expect("temp dir should be created");
        let path = temp_dir.path().to_owned();
        fs::write(path.join("scratch.txt"), b"scratch").unwrap();
        assert!(path.is_dir());
        path
    };

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_drop_logs_and_ignores_missing_directory() {
    ensure_test_logger();
    let dir = temp_dir("temp-dir-drop-missing");
    let temp_dir =
        LocalTempDir::in_dir(&dir, Some("drop-"), 4).expect("temp dir should be created");
    let path = temp_dir.path().to_owned();
    fs::remove_dir_all(&path).unwrap();

    drop(temp_dir);

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_persist_moves_directory() {
    let dir = temp_dir("temp-dir-persist");
    let temp_dir =
        LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("nested").join("persisted");
    fs::write(source.join("payload.txt"), b"payload").unwrap();

    let persisted = temp_dir.persist(&target).expect("temp dir should persist");

    assert_eq!(target, persisted);
    assert!(!source.exists());
    assert_eq!(
        b"payload",
        fs::read(target.join("payload.txt")).unwrap().as_slice()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_persist_cleans_up_when_parent_creation_fails() {
    let dir = temp_dir("temp-dir-persist-error");
    let temp_dir =
        LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let blocker = dir.join("blocker");
    fs::write(&blocker, b"not a directory").unwrap();

    let error = temp_dir
        .persist(blocker.join("target"))
        .expect_err("invalid parent should be returned");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_persist_cleans_up_when_target_exists() {
    let dir = temp_dir("temp-dir-persist-rename-error");
    let temp_dir =
        LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("target-file");
    fs::write(&target, b"not a directory").unwrap();

    let error = temp_dir
        .persist(&target)
        .expect_err("existing target should be rejected");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists
            | ErrorKind::NotADirectory
            | ErrorKind::PermissionDenied
            | ErrorKind::Other
    ));
    assert!(!source.exists());
    assert!(target.is_file());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_persist_returns_target_metadata_error() {
    let dir = temp_dir("temp-dir-persist-metadata-error");
    let temp_dir =
        LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("x".repeat(10_000));

    let error = temp_dir
        .persist(&target)
        .expect_err("target metadata error should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}
