// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;

#[cfg(all(coverage, target_os = "linux"))]
use super::super::test_support::run_in_coverage_fault_process;
#[cfg(target_os = "linux")]
use super::super::test_support::run_in_small_stack_process;
#[cfg(unix)]
use super::super::test_support::{PermissionsExt, short_temp_dir};

/// Asserts one injected directory-size traversal failure.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_dir_size_error(test_name: &str, fault: &str, nested: bool) {
    let Some(()) = run_in_coverage_fault_process(test_name, fault, move || {
        let dir = temp_dir(fault);
        if nested {
            fs::create_dir(dir.join("nested")).expect("nested directory should be created");
            fs::write(dir.join("nested/data.txt"), b"data").expect("nested file should be written");
        } else {
            fs::write(dir.join("data.txt"), b"data").expect("file fixture should be written");
        }

        let error = qubit_local_files::directory::size(&dir)
            .expect_err("injected directory-size traversal should fail");

        if fault.ends_with("overflow") {
            assert_eq!(ErrorKind::InvalidData, error.kind());
        } else if fault.ends_with("identity-cycle") {
            assert_eq!(ErrorKind::InvalidInput, error.kind());
        } else {
            assert_eq!(Some(libc::EIO), error.raw_os_error());
        }
        fs::remove_dir_all(dir).expect("test directory should be removed");
    }) else {
        return;
    };
}

/// Verifies propagation of injected directory-entry iteration failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_dir_size_reports_injected_entry_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::path_operations_tests::",
        "test_dir_size_reports_injected_entry_error",
    );
    assert_injected_dir_size_error(TEST_NAME, "dir-size-entry", false);
}

/// Verifies propagation of injected entry-metadata failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_dir_size_reports_injected_metadata_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::path_operations_tests::",
        "test_dir_size_reports_injected_metadata_error",
    );
    assert_injected_dir_size_error(TEST_NAME, "dir-size-metadata", false);
}

/// Verifies propagation of injected nested-directory open failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_dir_size_reports_injected_nested_read_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::path_operations_tests::",
        "test_dir_size_reports_injected_nested_read_error",
    );
    assert_injected_dir_size_error(TEST_NAME, "dir-size-read-dir", true);
}

/// Verifies cycle detection when distinct paths resolve to one directory
/// identity, as can happen through a bind mount.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_dir_size_rejects_injected_directory_identity_cycle() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::path_operations_tests::",
        "test_dir_size_rejects_injected_directory_identity_cycle",
    );
    assert_injected_dir_size_error(TEST_NAME, "dir-size-directory-identity-cycle", true);
}

/// Verifies normalization of injected file-size overflow.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_dir_size_reports_injected_file_overflow() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::path_operations_tests::",
        "test_dir_size_reports_injected_file_overflow",
    );
    assert_injected_dir_size_error(TEST_NAME, "dir-size-file-overflow", false);
}

/// Verifies normalization of injected child-directory size overflow.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_dir_size_reports_injected_directory_overflow() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::path_operations_tests::",
        "test_dir_size_reports_injected_directory_overflow",
    );
    assert_injected_dir_size_error(TEST_NAME, "dir-size-directory-overflow", true);
}
use super::super::test_support::{fs, temp_dir};

#[test]
fn test_ensure_dir_and_ensure_parent_create_missing_directories() {
    let dir = temp_dir("ensure");
    let child_dir = dir.join("a").join("b");
    let child_file = dir.join("c").join("d").join("out.txt");

    qubit_local_files::directory::create_all(&child_dir).expect("directory should be created");
    qubit_local_files::directory::create_parent(&child_file).expect("parent should be created");
    qubit_local_files::directory::create_parent(std::path::Path::new("parentless.txt"))
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
    std::os::unix::fs::symlink(dir.join("a.txt"), dir.join("link.txt")).unwrap();

    let size = qubit_local_files::directory::size(&dir).expect("directory size should be computed");

    assert_eq!(8, size);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_dir_size_handles_deep_tree_on_small_stack() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::path_operations_tests::",
        "test_dir_size_handles_deep_tree_on_small_stack",
    );
    const CHILD_ENVIRONMENT: &str = "QUBIT_LOCAL_FILES_DIR_SIZE_SMALL_STACK_CHILD";

    let Some(dir) = run_in_small_stack_process(TEST_NAME, CHILD_ENVIRONMENT, || {
        let dir =
            std::path::PathBuf::from(format!("/tmp/qio-{}-deep-dir-size", std::process::id(),));
        drop(fs::remove_dir_all(&dir));
        let root = dir.join("root");
        fs::create_dir_all(&root).expect("deep-tree root should be created");
        let mut current = root.clone();
        for _ in 0..512 {
            current.push("d");
            fs::create_dir(&current).expect("deep-tree directory should be created");
        }
        fs::write(current.join("leaf"), b"x").expect("deep-tree leaf should be written");

        assert_eq!(
            1,
            qubit_local_files::directory::size(&root).expect("deep-tree size should be computed"),
        );
        dir
    }) else {
        return;
    };

    fs::remove_dir_all(dir).expect("deep-tree fixture should be removed");
}

#[test]
fn test_dir_size_rejects_non_directory() {
    let dir = temp_dir("dir-size-error");
    let path = dir.join("file.txt");
    fs::write(&path, b"data").unwrap();

    let error = qubit_local_files::directory::size(&path)
        .expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_returns_missing_path_error() {
    let dir = temp_dir("dir-size-missing");
    let missing = dir.join("missing");

    let error = qubit_local_files::directory::size(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_dir_size_returns_read_dir_error() {
    let dir = temp_dir("dir-size-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error =
        qubit_local_files::directory::size(&dir).expect_err("unreadable directory should fail");

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
    std::os::unix::fs::symlink(dir.join("file.txt"), dir.join("link.txt")).unwrap();

    qubit_local_files::directory::clear(&dir).expect("directory should be cleaned");

    assert!(dir.is_dir());
    assert_eq!(0, fs::read_dir(&dir).unwrap().count());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_rejects_non_directory() {
    let dir = temp_dir("clean-dir-error");
    let path = dir.join("file.txt");
    fs::write(&path, b"data").unwrap();

    let error = qubit_local_files::directory::clear(&path)
        .expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_returns_missing_path_error() {
    let dir = temp_dir("clean-dir-missing");
    let missing = dir.join("missing");

    let error = qubit_local_files::directory::clear(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_clean_dir_returns_read_dir_error() {
    let dir = temp_dir("clean-dir-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error =
        qubit_local_files::directory::clear(&dir).expect_err("unreadable directory should fail");

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

    qubit_local_files::remove::any(&file).expect("file should be removed");
    qubit_local_files::remove::any(&nested).expect("directory should be removed");

    assert!(!file.exists());
    assert!(!nested.exists());

    #[cfg(unix)]
    {
        let target = dir.join("target.txt");
        let link = dir.join("link.txt");
        fs::write(&target, b"target").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        qubit_local_files::remove::any(&link).expect("symlink should be removed");

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

    qubit_local_files::remove::any(&link).expect("directory symlink should be removed");

    assert!(!link.exists());
    assert!(target.is_dir());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_remove_any_returns_missing_path_error() {
    let dir = temp_dir("remove-any-missing");
    let missing = dir.join("missing");

    let error =
        qubit_local_files::remove::any(&missing).expect_err("missing path should return an error");

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

    assert_eq!(3, qubit_local_files::directory::size(&dir).unwrap());

    drop(listener);
    fs::remove_dir_all(dir).unwrap();
}
