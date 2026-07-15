// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use super::test_support::PermissionsExt;
#[cfg(windows)]
use super::test_support::path_with_interior_nul;
use super::test_support::{
    ErrorKind,
    LocalPersistOptions,
    LocalTempFile,
    Seek,
    SeekFrom,
    Write,
    ensure_test_logger,
    fs,
    temp_dir,
};

#[cfg(unix)]
#[test]
fn test_temp_file_uses_private_permissions() {
    let file = LocalTempFile::new().expect("temporary file should be created");
    let mode = file
        .metadata()
        .expect("temporary file metadata should be readable")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(0o600, mode);
}

#[test]
fn test_temp_file_with_name_uses_system_temp_directory() {
    let file =
        LocalTempFile::with_name(Some("qubit-local-files-test-"), Some(".tmp"))
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
fn test_temp_file_exists_and_cleanup() {
    let dir = temp_dir("temp-file-cleanup");
    let file = LocalTempFile::in_dir(&dir, Some("cleanup-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    let path = file.path().to_owned();

    assert!(
        file.exists()
            .expect("temp file existence should be checked")
    );
    file.cleanup().expect("temp file should be cleaned up");

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_debug_formatting_contains_type_name() {
    let file = LocalTempFile::with_name(
        Some("qubit-local-files-debug-"),
        Some(".tmp"),
    )
    .expect("temp file should be created");

    assert!(format!("{file:?}").contains("LocalTempFile"));
}

#[test]
fn test_temp_file_metadata_and_close_handle() {
    let dir = temp_dir("temp-file-close");
    let mut file = LocalTempFile::in_dir(&dir, Some("close-"), Some(".tmp"), 4)
        .expect("temp file should be created");

    file.metadata().expect("metadata should be readable");
    file.close();
    file.close();
    let write_error = file
        .write_all(b"closed")
        .expect_err("closed temporary file should reject writes");
    let flush_error = file
        .flush()
        .expect_err("closed temporary file should reject flushes");
    let seek_error = file
        .seek(SeekFrom::Start(0))
        .expect_err("closed temporary file should reject seeks");

    assert_eq!(ErrorKind::NotFound, write_error.kind());
    assert_eq!(ErrorKind::NotFound, flush_error.kind());
    assert_eq!(ErrorKind::NotFound, seek_error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_cleanup_returns_missing_file_error() {
    let dir = temp_dir("temp-file-cleanup-missing");
    let file = LocalTempFile::in_dir(&dir, Some("cleanup-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    fs::remove_file(file.path()).expect("temporary file should be removed");

    let error = file
        .cleanup()
        .expect_err("cleanup should report a missing temporary file");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_close_releases_handle_and_rejects_writes_after_close() {
    let dir = temp_dir("temp-file-writer-close");
    let mut file =
        LocalTempFile::in_dir(&dir, Some("writer-"), Some(".tmp"), 4)
            .expect("temp file should be created");
    let path = file.path().to_owned();

    file.write_all(b"payload")
        .expect("payload should be written through the owned handle");
    file.flush()
        .expect("temporary file should flush through the write trait");
    file.close();
    let error = file
        .write_all(b"rejected")
        .expect_err("closed temporary file should reject writes");

    assert_eq!(b"payload", fs::read(&path).unwrap().as_slice());
    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_supports_seek_through_owned_handle() {
    let dir = temp_dir("temp-file-seek");
    let mut file = LocalTempFile::in_dir(&dir, Some("seek-"), Some(".tmp"), 4)
        .expect("temp file should be created");

    file.write_all(b"one-two")
        .expect("initial payload should be written");
    file.seek(SeekFrom::Start(3))
        .expect("temporary file should seek");
    file.write_all(b"+")
        .expect("payload should be overwritten after seeking");
    file.close();

    assert_eq!(b"one+two", fs::read(file.path()).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_exposes_owned_file_handle() {
    let dir = temp_dir("temp-file-handle");
    let mut file =
        LocalTempFile::in_dir(&dir, Some("handle-"), Some(".tmp"), 4)
            .expect("temp file should be created");

    file.as_file_mut()
        .expect("owned file handle should be available")
        .write_all(b"handle")
        .expect("owned file handle should be writable");
    let length = file
        .as_file()
        .expect("owned file handle should be available")
        .metadata()
        .expect("owned file metadata should be readable")
        .len();

    assert_eq!(6, length);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_new_creates_unique_existing_files() {
    let first_file =
        LocalTempFile::new().expect("first temp file should exist");
    let second_file =
        LocalTempFile::new().expect("second temp file should exist");
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
        LocalTempFile::in_dir(&dir, Some("local-"), Some(".tmp"), 4)
            .expect("first temp file should be created in dir");
    let second_file =
        LocalTempFile::in_dir(&dir, Some("local-"), Some(".tmp"), 4)
            .expect("second temp file should be created in dir");
    let first_path = first_file.path().to_owned();
    let second_path = second_file.path().to_owned();

    first_file.write_all(b"abc").unwrap();

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
    file.write_all(b"kept").unwrap();

    let path = file.keep();

    assert!(path.exists());
    assert_eq!(b"kept", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_moves_file() {
    let dir = temp_dir("temp-file-persist");
    let mut file =
        LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
            .expect("temp file should be created");
    file.write_all(b"payload").unwrap();
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
    let mut file =
        LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
            .expect("temp file should be created");
    file.write_all(b"new").unwrap();
    let source = file.path().to_owned();
    let target = dir.join("result.txt");
    fs::write(&target, b"old").unwrap();

    let error = file
        .persist(&target)
        .expect_err("existing target should be rejected by default");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    assert_eq!(b"old", fs::read(&target).unwrap().as_slice());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_with_overwrite_replaces_existing_target() {
    let dir = temp_dir("temp-file-persist-overwrite");
    let mut file =
        LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
            .expect("temp file should be created");
    file.write_all(b"new").unwrap();
    let source = file.path().to_owned();
    let target = dir.join("result.txt");
    fs::write(&target, b"old").unwrap();

    let persisted = file
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect("overwrite option should replace existing target");

    assert_eq!(target, persisted);
    assert!(!source.exists());
    assert_eq!(b"new", fs::read(&target).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_with_default_rejects_existing_target() {
    let dir = temp_dir("temp-file-persist-default-existing-target");
    let mut file =
        LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
            .expect("temp file should be created");
    file.write_all(b"new").unwrap();
    let source = file.path().to_owned();
    let target = dir.join("result.txt");
    fs::write(&target, b"old").unwrap();

    let error = file
        .persist_with(&target, LocalPersistOptions::default())
        .expect_err("default persist options should reject existing targets");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    assert_eq!(b"old", fs::read(&target).unwrap().as_slice());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_file_persist_rejects_target_with_nul_byte() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = temp_dir("temp-file-persist-nul-target");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    let source = file.path().to_owned();
    let target = dir.join(OsString::from_vec(b"bad\0target.txt".to_vec()));

    let error = file
        .persist(&target)
        .expect_err("NUL target should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn test_temp_file_persist_rejects_windows_target_with_nul_byte() {
    let dir = temp_dir("temp-file-persist-windows-nul-target");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    let source = file.path().to_owned();
    let prefix = dir.join("unexpected-target");
    let target = path_with_interior_nul(&dir, "unexpected-target");

    let error = file
        .persist(&target)
        .expect_err("Windows NUL target should be rejected before moving");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    assert!(!prefix.exists(), "NUL prefix target must not be created");
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_temp_file_persist_with_overwrite_rejects_windows_target_with_nul_byte()
{
    let dir = temp_dir("temp-file-overwrite-windows-nul-target");
    let mut file =
        LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
            .expect("temp file should be created");
    file.write_all(b"replacement")
        .expect("temporary contents should be written");
    let source = file.path().to_owned();
    let prefix = dir.join("existing-target");
    fs::write(&prefix, b"original").expect("prefix target should be written");
    let target = path_with_interior_nul(&dir, "existing-target");

    let error = file
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect_err("Windows NUL target should be rejected before replacement");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    assert_eq!(b"original", fs::read(&prefix).unwrap().as_slice());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_temp_file_persist_returns_target_metadata_error() {
    let dir = temp_dir("temp-file-persist-metadata-error");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    let source = file.path().to_owned();
    let target = dir.join("x".repeat(10_000));

    let error = file
        .persist(&target)
        .expect_err("target metadata error should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_persist_returns_resource_when_parent_creation_fails() {
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
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}
