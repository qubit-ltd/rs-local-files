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
    FileBuffering,
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalTempDir,
    Read,
    Write,
    ensure_test_logger,
    fs,
    temp_dir,
};

#[test]
fn test_debug_formatting_contains_type_name() {
    let dir = LocalTempDir::with_prefix(Some("qubit-local-files-debug-")).expect("temp directory should be created");

    assert!(format!("{dir:?}").contains("LocalTempDir"));
}

#[test]
fn test_temp_dir_with_prefix_creates_existing_directory() {
    let dir = LocalTempDir::with_prefix(Some("qubit-local-files-dir-")).expect("temp directory should be created");
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
fn test_temp_dir_exists_metadata_and_cleanup() {
    let dir = temp_dir("temp-dir-cleanup");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("cleanup-"), 4).expect("temp dir should be created");
    let path = temp_dir.path().to_owned();

    assert!(temp_dir.exists().expect("temp dir existence should be checked"));
    assert!(temp_dir.metadata().expect("metadata should be read").is_dir());
    temp_dir.cleanup().expect("temp dir should be cleaned up");

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
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

    let error =
        LocalTempDir::in_dir(&dir, Some("missing-parent/"), 1).expect_err("path-like prefix should be rejected");

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

    let error =
        LocalTempDir::in_dir(&dir, Some("local-"), 1).expect_err("unwritable directory should return create-dir error");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_child_path_rejects_escape_and_ensure_child_dir_creates_parents() {
    let dir = temp_dir("temp-dir-child-path");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");

    let child = temp_dir
        .child_path("a/b/c.txt")
        .expect("nested child path should be accepted");
    let ensured = temp_dir
        .ensure_child_dir("a/b/nested")
        .expect("nested child directory should be created with parents");
    let error = temp_dir
        .child_path("../outside.txt")
        .expect_err("parent traversal should be rejected");

    assert_eq!(temp_dir.path().join("a/b/c.txt"), child);
    assert!(ensured.is_dir());
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_child_path_rejects_empty_path() {
    let dir = temp_dir("temp-dir-empty-child");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");

    let error = temp_dir
        .child_path("")
        .expect_err("empty child path should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_ensure_child_dir_rejects_existing_file_component() {
    let dir = temp_dir("temp-dir-child-file-component");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");
    fs::write(temp_dir.path().join("blocker"), b"not a directory").unwrap();

    let error = temp_dir
        .ensure_child_dir("blocker/nested")
        .expect_err("file path component should be rejected");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_ensure_child_dir_returns_metadata_error() {
    let dir = temp_dir("temp-dir-child-metadata-error");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");
    let long_name = "x".repeat(10_000);

    let error = temp_dir
        .ensure_child_dir(long_name)
        .expect_err("filesystem metadata errors should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_ensure_child_dir_rejects_symlink_component() {
    let dir = temp_dir("temp-dir-child-symlink-component");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");
    let target = dir.join("target");
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, temp_dir.path().join("link")).unwrap();

    let error = temp_dir
        .ensure_child_dir("link/nested")
        .expect_err("symlink path component should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_list_and_child_reader_writer_use_shared_options() {
    let dir = temp_dir("temp-dir-child-io");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");
    let child = "nested/data.txt";

    {
        let mut writer = temp_dir
            .open_child_writer(
                child,
                FileWriteOptions {
                    create_parent: true,
                    mode: FileWriteMode::CreateNew,
                    buffering: FileBuffering::Buffered { capacity: Some(8) },
                },
            )
            .expect("child writer should create parent directories");
        writer.write_all(b"payload").unwrap();
        writer.close().unwrap();
    }

    let mut reader = temp_dir
        .open_child_reader(
            child,
            FileReadOptions {
                buffering: FileBuffering::Buffered { capacity: None },
            },
        )
        .expect("child reader should open a child file");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    let mut entries = temp_dir
        .list()
        .expect("temp directory should be listed")
        .map(|entry| entry.expect("entry should be readable").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    let error = temp_dir
        .open_child_reader("nested", FileReadOptions::default())
        .expect_err("child reader should reject directories");

    assert_eq!(b"payload", content.as_slice());
    assert_eq!(vec![std::ffi::OsString::from("nested")], entries);
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_open_child_writer_validates_existing_parent_and_target() {
    let dir = temp_dir("temp-dir-child-writer-validation");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");
    temp_dir.ensure_child_dir("nested").expect("parent should be created");
    fs::write(temp_dir.path().join("nested/existing.txt"), b"old").unwrap();

    {
        let mut writer = temp_dir
            .open_child_writer(
                "nested/existing.txt",
                FileWriteOptions::new(FileWriteMode::AppendExisting),
            )
            .expect("existing child file should open for append");
        writer.write_all(b"-new").unwrap();
        writer.close().unwrap();
    }

    let missing_parent_error = temp_dir
        .open_child_writer("missing/file.txt", FileWriteOptions::default())
        .expect_err("missing parent should be rejected without create_parent");
    let directory_error = temp_dir
        .open_child_writer("nested", FileWriteOptions::default())
        .expect_err("directory target should be rejected");

    assert_eq!(
        b"old-new",
        fs::read(temp_dir.path().join("nested/existing.txt"))
            .unwrap()
            .as_slice()
    );
    assert_eq!(ErrorKind::NotFound, missing_parent_error.kind());
    assert_eq!(ErrorKind::InvalidInput, directory_error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_open_child_writer_returns_metadata_error() {
    let dir = temp_dir("temp-dir-child-writer-metadata-error");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");
    let long_name = "x".repeat(10_000);

    let error = temp_dir
        .open_child_writer(long_name, FileWriteOptions::default())
        .expect_err("filesystem metadata errors should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_child_reader_rejects_symlink_escape() {
    let dir = temp_dir("temp-dir-symlink-escape");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4).expect("temp dir should be created");
    let outside = dir.join("outside.txt");
    fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, temp_dir.path().join("link.txt")).unwrap();

    let error = temp_dir
        .open_child_reader("link.txt", FileReadOptions::default())
        .expect_err("child symlink escaping the temp directory should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_drop_removes_directory_tree() {
    let dir = temp_dir("temp-dir-drop");
    let path = {
        let temp_dir = LocalTempDir::in_dir(&dir, Some("drop-"), 4).expect("temp dir should be created");
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("drop-"), 4).expect("temp dir should be created");
    let path = temp_dir.path().to_owned();
    fs::remove_dir_all(&path).unwrap();

    drop(temp_dir);

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_persist_moves_directory() {
    let dir = temp_dir("temp-dir-persist");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("nested").join("persisted");
    fs::write(source.join("payload.txt"), b"payload").unwrap();

    let persisted = temp_dir.persist(&target).expect("temp dir should persist");

    assert_eq!(target, persisted);
    assert!(!source.exists());
    assert_eq!(b"payload", fs::read(target.join("payload.txt")).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_persist_cleans_up_when_parent_creation_fails() {
    let dir = temp_dir("temp-dir-persist-error");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("target-file");
    fs::write(&target, b"not a directory").unwrap();

    let error = temp_dir
        .persist(&target)
        .expect_err("existing target should be rejected");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory | ErrorKind::PermissionDenied | ErrorKind::Other
    ));
    assert!(!source.exists());
    assert!(target.is_file());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_persist_returns_target_metadata_error() {
    let dir = temp_dir("temp-dir-persist-metadata-error");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4).expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("x".repeat(10_000));

    let error = temp_dir
        .persist(&target)
        .expect_err("target metadata error should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}
