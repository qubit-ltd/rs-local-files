// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
// =============================================================================

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalReadAllOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalSnapshotOptions;
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

const HOST_CHILD_FIXTURE: &str = "QUBIT_READ_TEXT_HOST_FIXTURE";

#[test]
fn test_rooted_invalid_utf8_keeps_bound_path_and_pwd() {
    let scratch = tempdir().expect("scratch");
    fs::create_dir(scratch.path().join("sub")).expect("sub");
    fs::write(scratch.path().join("sub/bad.txt"), [0xff]).expect("bytes");
    let mut filesystem = LocalFileSystem::rooted(scratch.path()).expect("rooted");
    filesystem.set_current_directory(Path::new("/sub")).expect("pwd");

    let error = filesystem.read_text(Path::new("bad.txt")).expect_err("UTF8");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidEncoding);
    assert_eq!(error.path(), Some(Path::new("/sub/bad.txt")));
    assert_eq!(error.current_directory(), Some(Path::new("/sub")));
    assert!(Error::source(&error).is_some());
}

#[test]
fn test_rooted_read_limits_keep_bound_path_and_pwd() {
    let scratch = tempdir().expect("scratch");
    fs::create_dir(scratch.path().join("sub")).expect("sub");
    fs::write(scratch.path().join("sub/local"), b"hello").expect("content");
    let mut filesystem = LocalFileSystem::rooted(scratch.path()).expect("rooted");
    filesystem.set_current_directory(Path::new("/sub")).expect("pwd");

    let errors = [
        filesystem
            .read_all_with_options(Path::new("local"), &LocalReadAllOptions::new().with_max_bytes(1))
            .expect_err("read-all should exceed one byte"),
        filesystem
            .read_text_with_options(Path::new("local"), &LocalReadAllOptions::new().with_max_bytes(1))
            .expect_err("read-text should exceed one byte"),
        filesystem
            .read_consistent_with_options(Path::new("local"), &LocalSnapshotOptions::builder(1).build())
            .expect_err("snapshot should exceed one byte"),
    ];
    for error in errors {
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.operation(), LocalFileOperation::Read);
        assert_eq!(error.path(), Some(Path::new("/sub/local")));
        assert_eq!(error.current_directory(), Some(Path::new("/sub")));
    }
}

#[test]
fn test_host_relative_invalid_utf8_keeps_first_pwd_in_subprocess() {
    if let Some(fixture) = std::env::var_os(HOST_CHILD_FIXTURE) {
        let fixture = std::path::PathBuf::from(fixture);
        #[cfg(windows)]
        let fixture = {
            use std::os::windows::ffi::OsStrExt;
            use std::os::windows::ffi::OsStringExt;

            let path = fixture.as_os_str().encode_wide().collect::<Vec<_>>();
            if path.starts_with(&[92, 92, 63, 92]) {
                std::path::PathBuf::from(std::ffi::OsString::from_wide(&path[4..]))
            } else {
                fixture
            }
        };
        let expected_path = fixture.join("bad.txt");
        let filesystem = LocalFileSystem::host().expect("Host");
        let error = filesystem.read_text(Path::new("bad.txt")).expect_err("UTF8");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidEncoding);
        assert_eq!(error.path(), Some(expected_path.as_path()));
        assert_eq!(error.current_directory(), Some(fixture.as_path()));
        assert!(Error::source(&error).is_some());
        let filesystem = LocalFileSystem::host().expect("Host");
        let errors = [
            filesystem
                .read_all_with_options(Path::new("bad.txt"), &LocalReadAllOptions::new().with_max_bytes(0))
                .expect_err("read-all should exceed zero bytes"),
            filesystem
                .read_text_with_options(Path::new("bad.txt"), &LocalReadAllOptions::new().with_max_bytes(0))
                .expect_err("read-text should exceed zero bytes"),
            filesystem
                .read_consistent_with_options(Path::new("bad.txt"), &LocalSnapshotOptions::builder(0).build())
                .expect_err("snapshot should exceed zero bytes"),
        ];
        for error in errors {
            assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
            assert_eq!(error.path(), Some(expected_path.as_path()));
            assert_eq!(error.current_directory(), Some(fixture.as_path()));
        }
        println!("READ_TEXT_HOST_CONTEXT_COMPLETE");
        return;
    }

    let scratch = tempdir().expect("scratch");
    let root = fs::canonicalize(scratch.path()).expect("scratch root should resolve");
    fs::write(root.join("bad.txt"), [0xff]).expect("bytes");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("test_host_relative_invalid_utf8_keeps_first_pwd_in_subprocess")
        .arg("--nocapture")
        .current_dir(&root)
        .env(HOST_CHILD_FIXTURE, &root)
        .output()
        .expect("child test should start");
    assert!(
        output.status.success(),
        "child test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("READ_TEXT_HOST_CONTEXT_COMPLETE"));
}

#[test]
fn test_absolute_host_invalid_utf8_does_not_require_pwd() {
    let scratch = tempdir().expect("scratch");
    let path = scratch.path().join("bad.txt");
    fs::write(&path, [0xff]).expect("bytes");
    let filesystem = LocalFileSystem::host().expect("Host");

    let error = filesystem.read_text(&path).expect_err("UTF8");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidEncoding);
    assert_eq!(error.path(), Some(path.as_path()));
    assert_eq!(error.current_directory(), None);
    assert!(Error::source(&error).is_some());

    let limited = filesystem
        .read_all_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(0))
        .expect_err("read-all limit");
    assert_eq!(limited.operation(), LocalFileOperation::Read);
    assert_eq!(limited.path(), Some(path.as_path()));
    assert_eq!(limited.current_directory(), None);

    let limited = filesystem
        .read_text_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(0))
        .expect_err("read-text limit");
    assert_eq!(limited.operation(), LocalFileOperation::Read);
    assert_eq!(limited.path(), Some(path.as_path()));
    assert_eq!(limited.current_directory(), None);

    let limited = filesystem
        .read_consistent_with_options(&path, &LocalSnapshotOptions::builder(0).build())
        .expect_err("snapshot limit");
    assert_eq!(limited.operation(), LocalFileOperation::Read);
    assert_eq!(limited.path(), Some(path.as_path()));
    assert_eq!(limited.current_directory(), None);
}

#[test]
fn test_read_text_preserves_complete_read_options_and_utf8_behavior() {
    let scratch = tempdir().expect("scratch");
    let path = scratch.path().join("text.txt");
    fs::write(&path, b"hello").expect("bytes");
    let filesystem = LocalFileSystem::host().expect("Host");

    assert_eq!(filesystem.read_text(&path).expect("valid UTF-8"), "hello");
    let limited = filesystem
        .read_text_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(4))
        .expect_err("byte limit");
    assert_eq!(limited.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(limited.path(), Some(path.as_path()));
}

#[cfg(unix)]
#[test]
fn test_read_text_inherits_filesystem_read_options() {
    use std::os::unix::fs::symlink;

    let scratch = tempdir().expect("scratch");
    fs::write(scratch.path().join("target.txt"), b"hello").expect("target");
    symlink("target.txt", scratch.path().join("link.txt")).expect("symlink");
    let mut filesystem = LocalFileSystem::rooted(scratch.path()).expect("rooted");
    filesystem
        .set_default_read_options(LocalReadOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject))
        .expect("default reader options");

    let error = filesystem
        .read_text_with_options(Path::new("/link.txt"), &LocalReadAllOptions::new())
        .expect_err("inherited options should reject symbolic links");
    assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);
}
