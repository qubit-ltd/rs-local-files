// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use super::test_support::PermissionsExt;
use super::test_support::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
    ErrorKind,
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
fn test_temp_dir_exposes_absolute_location_after_cwd_change() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .expect("current directory lock should be acquired");
    let dir = temp_dir("temp-dir-absolute-cwd");
    let creation_dir = dir.join("creation");
    let later_dir = dir.join("later");
    fs::create_dir_all(creation_dir.join("temp"))
        .expect("relative creation parent should be created");
    fs::create_dir_all(&later_dir).expect("later directory should be created");

    let (
        path_is_absolute,
        path_has_expected_parent,
        exists_result,
        child_result,
        expected_child_path,
        nested_exists_before_drop,
        path_exists_after_drop,
    ) = {
        let _guard = CurrentDirGuard::change_to(&creation_dir);
        let temp_dir = LocalTempDir::in_dir("temp", Some("cwd-"), 4)
            .expect("relative temporary directory should be created");
        let path_is_absolute = temp_dir.path().is_absolute();
        let path_has_expected_parent =
            temp_dir.path().starts_with(creation_dir.join("temp"));
        let generated_path = temp_dir.path().to_owned();
        let expected_child_path = generated_path.join("nested");
        std::env::set_current_dir(&later_dir)
            .expect("current directory should change");
        let exists_result = temp_dir.exists();
        let child_result = temp_dir.ensure_child_dir("nested");
        let nested_exists_before_drop = generated_path.join("nested").exists();
        drop(temp_dir);
        (
            path_is_absolute,
            path_has_expected_parent,
            exists_result,
            child_result,
            expected_child_path,
            nested_exists_before_drop,
            generated_path.exists(),
        )
    };
    fs::remove_dir_all(&dir).expect("temporary fixture should be removed");

    assert!(path_is_absolute);
    assert!(path_has_expected_parent);
    assert!(exists_result.expect("existence should be checked"));
    assert_eq!(
        expected_child_path,
        child_result.expect("absolute child directory should be created")
    );
    assert!(nested_exists_before_drop);
    assert!(!path_exists_after_drop);
}

#[test]
fn test_debug_formatting_contains_type_name() {
    let dir = LocalTempDir::with_prefix("qubit-local-files-debug-")
        .expect("temp directory should be created");

    assert!(format!("{dir:?}").contains("LocalTempDir"));
}

#[test]
fn test_temp_dir_with_prefix_creates_existing_directory() {
    let dir = LocalTempDir::with_prefix("qubit-local-files-dir-")
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

#[cfg(unix)]
#[test]
fn test_temp_dir_uses_private_permissions() {
    let dir =
        LocalTempDir::new().expect("temporary directory should be created");
    let mode = dir
        .metadata()
        .expect("temporary directory metadata should be readable")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(0o700, mode);
}

#[cfg(unix)]
#[test]
fn test_temp_dir_child_directory_uses_private_permissions() {
    let dir =
        LocalTempDir::new().expect("temporary directory should be created");
    let child = dir
        .ensure_child_dir("nested")
        .expect("child directory should be created");
    let mode = fs::metadata(child)
        .expect("child directory metadata should be readable")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(0o700, mode);
}

#[test]
fn test_temp_dir_exists_metadata_and_cleanup() {
    let dir = temp_dir("temp-dir-cleanup");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("cleanup-"), 4)
        .expect("temp dir should be created");
    let path = temp_dir.path().to_owned();

    assert!(
        temp_dir
            .exists()
            .expect("temp dir existence should be checked")
    );
    assert!(
        temp_dir
            .metadata()
            .expect("metadata should be read")
            .is_dir()
    );
    temp_dir.cleanup().expect("temp dir should be cleaned up");

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_cleanup_returns_missing_directory_error() {
    let dir = temp_dir("temp-dir-cleanup-missing");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("cleanup-"), 4)
        .expect("temporary directory should be created");
    fs::remove_dir_all(temp_dir.path())
        .expect("temporary directory should be removed externally");

    let error = temp_dir
        .cleanup()
        .expect_err("cleanup should report a missing temporary directory");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).expect("temporary fixture should be removed");
}

#[test]
fn test_temp_dir_new_and_keep_preserves_directory() {
    let dir = LocalTempDir::new().expect("temp directory should be created");
    let path = dir.keep();

    assert!(path.is_dir());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn test_temp_dir_keep_returns_absolute_path() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .expect("current directory lock should be acquired");
    let dir = temp_dir("temp-dir-keep-absolute");
    let creation_dir = dir.join("creation");
    let later_dir = dir.join("later");
    fs::create_dir_all(creation_dir.join("temp"))
        .expect("relative creation parent should be created");
    fs::create_dir_all(&later_dir).expect("later directory should be created");

    let (kept_path, kept_path_exists) = {
        let _guard = CurrentDirGuard::change_to(&creation_dir);
        let temp_dir = LocalTempDir::in_dir("temp", Some("keep-"), 4)
            .expect("relative temporary directory should be created");
        let kept_path = temp_dir.keep();
        std::env::set_current_dir(&later_dir)
            .expect("current directory should change");
        let kept_path_exists = kept_path.exists();
        (kept_path, kept_path_exists)
    };
    fs::remove_dir_all(&dir).expect("temporary fixture should be removed");

    assert!(kept_path.is_absolute());
    assert!(kept_path.starts_with(creation_dir.join("temp")));
    assert!(kept_path_exists);
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

    let error = LocalTempDir::in_dir(&dir, None, 0)
        .expect_err("zero retries should be invalid");

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
fn test_temp_dir_child_path_rejects_escape_and_ensure_child_dir_creates_parents()
 {
    let dir = temp_dir("temp-dir-child-path");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");

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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");

    let error = temp_dir
        .child_path("")
        .expect_err("empty child path should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_child_path_rejects_explicit_dot_components() {
    let dir = temp_dir("temp-dir-explicit-dot-child");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");

    for child in ["a/./child.txt", "a/."] {
        let error = temp_dir
            .child_path(child)
            .expect_err("explicit dot components should be rejected");
        assert_eq!(ErrorKind::InvalidInput, error.kind());
    }

    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_temp_dir_child_path_rejects_unix_nul() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = temp_dir("temp-dir-unix-nul-child");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    let child = OsString::from_vec(b"safe\0unsafe".to_vec());
    let error = temp_dir
        .child_path(child)
        .expect_err("NUL should be rejected before filesystem use");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_temp_dir_child_path_rejects_windows_nul() {
    use std::path::Path;

    use super::test_support::path_with_interior_nul;

    let dir = temp_dir("temp-dir-windows-nul-child");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    let child = path_with_interior_nul(Path::new("nested"), "unsafe");
    let error = temp_dir
        .child_path(child)
        .expect_err("NUL should be rejected before filesystem use");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_temp_dir_child_io_rejects_unsafe_paths() {
    let dir = temp_dir("temp-dir-unsafe-child-io");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");

    let read_error = temp_dir
        .open_child_reader("../outside.txt", FileReadOptions::default())
        .expect_err("unsafe reader path should be rejected");
    let write_error = temp_dir
        .open_child_writer("../outside.txt", FileWriteOptions::default())
        .expect_err("unsafe writer path should be rejected");

    assert_eq!(ErrorKind::InvalidInput, read_error.kind());
    assert_eq!(ErrorKind::InvalidInput, write_error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_ensure_child_dir_rejects_existing_file_component() {
    let dir = temp_dir("temp-dir-child-file-component");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    let child = "nested/data.txt";

    {
        let mut writer = temp_dir
            .open_child_writer(
                child,
                FileWriteOptions::new(FileWriteMode::CreateNew)
                    .with_parent()
                    .buffered_with_capacity(8)
                    .expect("positive buffer capacity should be accepted"),
            )
            .expect("child writer should create parent directories");
        writer.write_all(b"payload").unwrap();
        writer.close().unwrap();
    }

    let mut reader = temp_dir
        .open_child_reader(child, FileReadOptions::buffered())
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    temp_dir
        .ensure_child_dir("nested")
        .expect("parent should be created");
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    let long_name = "x".repeat(10_000);

    let error = temp_dir
        .open_child_writer(long_name, FileWriteOptions::default())
        .expect_err("filesystem metadata errors should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_open_child_writer_rejects_dangling_symlink_escape() {
    let dir = temp_dir("temp-dir-dangling-writer-symlink");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    let outside = dir.join("outside.txt");
    let link = temp_dir.path().join("link.txt");
    std::os::unix::fs::symlink(&outside, &link)
        .expect("dangling symlink should be created");

    let error = temp_dir
        .open_child_writer("link.txt", FileWriteOptions::default())
        .expect_err("dangling final symlink should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(!outside.exists(), "outside target must not be created");
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_temp_dir_child_reader_rejects_symlink_escape() {
    let dir = temp_dir("temp-dir-symlink-escape");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    let outside = dir.join("outside.txt");
    fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, temp_dir.path().join("link.txt"))
        .unwrap();

    let error = temp_dir
        .open_child_reader("link.txt", FileReadOptions::default())
        .expect_err(
            "child symlink escaping the temp directory should be rejected",
        );

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_drop_removes_directory_tree() {
    let dir = temp_dir("temp-dir-drop");
    let path = {
        let temp_dir = LocalTempDir::in_dir(&dir, Some("drop-"), 4)
            .expect("temp dir should be created");
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
    let temp_dir = LocalTempDir::in_dir(&dir, Some("drop-"), 4)
        .expect("temp dir should be created");
    let path = temp_dir.path().to_owned();
    fs::remove_dir_all(&path).unwrap();

    drop(temp_dir);

    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_persist_moves_directory() {
    let dir = temp_dir("temp-dir-persist");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4)
        .expect("temp dir should be created");
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
fn test_temp_dir_persist_returns_absolute_path() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .expect("current directory lock should be acquired");
    let dir = temp_dir("temp-dir-persist-absolute");
    let creation_dir = dir.join("creation");
    let later_dir = dir.join("later");
    fs::create_dir_all(creation_dir.join("temp"))
        .expect("relative creation parent should be created");
    fs::create_dir_all(&later_dir).expect("later directory should be created");

    let (persisted_path, persisted_path_exists) = {
        let _guard = CurrentDirGuard::change_to(&creation_dir);
        let temp_dir = LocalTempDir::in_dir("temp", Some("source-"), 4)
            .expect("relative temporary directory should be created");
        let persisted_path = temp_dir
            .persist("persisted/final-directory")
            .expect("relative target should be persisted");
        std::env::set_current_dir(&later_dir)
            .expect("current directory should change");
        let persisted_path_exists = persisted_path.exists();
        (persisted_path, persisted_path_exists)
    };
    fs::remove_dir_all(&dir).expect("temporary fixture should be removed");

    assert!(persisted_path.is_absolute());
    assert_eq!(
        creation_dir.join("persisted/final-directory"),
        persisted_path
    );
    assert!(persisted_path_exists);
}

#[cfg(unix)]
#[test]
fn test_temp_dir_persist_returns_resource_when_current_dir_is_unavailable() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .expect("current directory lock should be acquired");
    let dir = temp_dir("temp-dir-persist-unavailable-cwd");
    let removed_cwd = dir.join("removed-cwd");
    fs::create_dir(&removed_cwd)
        .expect("temporary current directory should exist");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4)
        .expect("temporary directory should be created");
    let source = temp_dir.path().to_owned();

    let error = {
        let _guard = CurrentDirGuard::change_to(&removed_cwd);
        fs::remove_dir(&removed_cwd)
            .expect("Unix should allow removing the process current directory");
        temp_dir.persist("relative-target").expect_err(
            "relative target should require a readable current directory",
        )
    };

    assert_eq!(ErrorKind::NotFound, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).expect("temporary fixture should be removed");
}

#[test]
fn test_temp_dir_persist_returns_resource_when_parent_creation_fails() {
    let dir = temp_dir("temp-dir-persist-error");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4)
        .expect("temp dir should be created");
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
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_dir_persist_returns_resource_when_target_exists() {
    let dir = temp_dir("temp-dir-persist-rename-error");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4)
        .expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("target-file");
    fs::write(&target, b"not a directory").unwrap();

    let error = temp_dir
        .persist(&target)
        .expect_err("existing target should be rejected");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    assert!(target.is_file());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_temp_dir_persist_returns_target_metadata_error() {
    let dir = temp_dir("temp-dir-persist-metadata-error");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("source-"), 4)
        .expect("temp dir should be created");
    let source = temp_dir.path().to_owned();
    let target = dir.join("x".repeat(10_000));

    let error = temp_dir
        .persist(&target)
        .expect_err("target metadata error should be returned");

    assert_ne!(ErrorKind::NotFound, error.kind());
    assert_eq!(source, error.resource.path());
    assert!(source.exists());
    drop(error);
    assert!(!source.exists());
    fs::remove_dir_all(dir).unwrap();
}
