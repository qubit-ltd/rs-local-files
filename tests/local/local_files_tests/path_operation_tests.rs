// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalFiles;
use std::io::ErrorKind;

#[cfg(windows)]
use super::super::test_support::path_with_interior_nul;
#[cfg(unix)]
use super::super::test_support::{
    PermissionsExt,
    short_temp_dir,
};
use super::super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_ensure_dir_and_ensure_parent_create_missing_directories() {
    let dir = temp_dir("ensure");
    let child_dir = dir.join("a").join("b");
    let child_file = dir.join("c").join("d").join("out.txt");

    LocalFiles::ensure_dir(&child_dir).expect("directory should be created");
    LocalFiles::ensure_parent(&child_file).expect("parent should be created");
    LocalFiles::ensure_parent("parentless.txt")
        .expect("a parentless path should require no directory creation");

    assert!(child_dir.is_dir());
    assert!(child_file.parent().unwrap().is_dir());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_sums_regular_files_and_ignores_symlinks() {
    let dir = temp_dir("dir-size");
    fs::create_dir(dir.join("nested")).unwrap();
    fs::write(dir.join("a.txt"), b"abc").unwrap();
    fs::write(dir.join("nested").join("b.txt"), b"12345").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("a.txt"), dir.join("link.txt"))
        .unwrap();

    let size =
        LocalFiles::dir_size(&dir).expect("directory size should be computed");

    assert_eq!(8, size);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_rejects_non_directory() {
    let dir = temp_dir("dir-size-error");
    let path = dir.join("file.txt");
    fs::write(&path, b"data").unwrap();

    let error = LocalFiles::dir_size(&path)
        .expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_returns_missing_path_error() {
    let dir = temp_dir("dir-size-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::dir_size(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_dir_size_returns_read_dir_error() {
    let dir = temp_dir("dir-size-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::dir_size(&dir)
        .expect_err("unreadable directory should fail");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_removes_children_and_keeps_directory() {
    let dir = temp_dir("clean-dir");
    fs::create_dir(dir.join("nested")).unwrap();
    fs::write(dir.join("nested").join("child.txt"), b"child").unwrap();
    fs::write(dir.join("file.txt"), b"file").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("file.txt"), dir.join("link.txt"))
        .unwrap();

    LocalFiles::clean_dir(&dir).expect("directory should be cleaned");

    assert!(dir.is_dir());
    assert_eq!(0, fs::read_dir(&dir).unwrap().count());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_rejects_non_directory() {
    let dir = temp_dir("clean-dir-error");
    let path = dir.join("file.txt");
    fs::write(&path, b"data").unwrap();

    let error = LocalFiles::clean_dir(&path)
        .expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_returns_missing_path_error() {
    let dir = temp_dir("clean-dir-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::clean_dir(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_clean_dir_returns_read_dir_error() {
    let dir = temp_dir("clean-dir-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::clean_dir(&dir)
        .expect_err("unreadable directory should fail");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_remove_any_removes_files_directories_and_symlinks() {
    let dir = temp_dir("remove-any");
    let file = dir.join("file.txt");
    let nested = dir.join("nested");
    fs::write(&file, b"file").unwrap();
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("child.txt"), b"child").unwrap();

    LocalFiles::remove_any(&file).expect("file should be removed");
    LocalFiles::remove_any(&nested).expect("directory should be removed");

    assert!(!file.exists());
    assert!(!nested.exists());

    #[cfg(unix)]
    {
        let target = dir.join("target.txt");
        let link = dir.join("link.txt");
        fs::write(&target, b"target").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        LocalFiles::remove_any(&link).expect("symlink should be removed");

        assert!(target.exists());
        assert!(!link.exists());
    }

    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn test_remove_any_removes_directory_symlink_without_removing_target() {
    use std::os::windows::fs::symlink_dir;

    let dir = temp_dir("remove-directory-symlink");
    let target = dir.join("target");
    let link = dir.join("link");
    fs::create_dir_all(&target).expect("target directory should be created");
    if let Err(error) = symlink_dir(&target, &link) {
        assert_eq!(ErrorKind::PermissionDenied, error.kind());
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }

    LocalFiles::remove_any(&link).expect("directory symlink should be removed");

    assert!(!link.exists());
    assert!(target.is_dir());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_remove_any_returns_missing_path_error() {
    let dir = temp_dir("remove-any-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::remove_any(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_dir_size_ignores_unsupported_directory_entries() {
    use std::os::unix::net::UnixListener;

    let dir = short_temp_dir("dir-size-socket-entry");
    fs::write(dir.join("data.bin"), b"abc").unwrap();
    let listener = UnixListener::bind(dir.join("socket")).unwrap();

    assert_eq!(3, LocalFiles::dir_size(&dir).unwrap());

    drop(listener);
    fs::remove_dir_all(dir).unwrap();
}
