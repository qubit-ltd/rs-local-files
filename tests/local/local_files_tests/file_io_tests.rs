// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::{
    directory,
    metadata,
    read,
    write,
};
use std::io::{
    Error,
    ErrorKind,
    Read,
    Write,
};
#[cfg(unix)]
use std::os::unix::fs::symlink;

#[cfg(unix)]
use super::super::test_support::PermissionsExt;
#[cfg(all(coverage, target_os = "linux"))]
use super::super::test_support::run_in_coverage_fault_process;
use super::super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_open_helpers_reject_directories() {
    let directory = temp_dir("non-file-open");
    let read_error = read::open(&directory)
        .expect_err("directory reader should be rejected");
    assert_eq!(ErrorKind::InvalidInput, read_error.kind());

    let write_error = write::open(&directory, &write::OpenOptions::default())
        .expect_err("directory writer should be rejected");
    assert_eq!(ErrorKind::InvalidInput, write_error.kind());
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_file_handle_error(
    test_name: &str,
    fault: &str,
    expected_kind: ErrorKind,
) {
    let Some(()) = run_in_coverage_fault_process(test_name, fault, move || {
        let directory = temp_dir(fault);
        let path = directory.join("data.txt");
        fs::write(&path, b"data").expect("fixture should be written");
        let error =
            read::open(&path).expect_err("injected validation should fail");
        assert_eq!(expected_kind, error.kind());
        fs::remove_dir_all(directory).unwrap();
    }) else {
        return;
    };
}

#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_reader_reports_injected_file_handle_metadata_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::file_io_tests::",
        "test_open_reader_reports_injected_file_handle_metadata_error",
    );
    assert_injected_file_handle_error(
        TEST_NAME,
        "file-handle-metadata",
        ErrorKind::Other,
    );
}

#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_reader_reports_injected_file_handle_type_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::file_io_tests::",
        "test_open_reader_reports_injected_file_handle_type_error",
    );
    assert_injected_file_handle_error(
        TEST_NAME,
        "file-handle-type",
        ErrorKind::InvalidInput,
    );
}

#[test]
fn test_open_reader_and_writer_replace_old_buffered_helpers() {
    let dir = temp_dir("buffered");
    let path = dir.join("a").join("b").join("data.txt");

    {
        let mut writer = write::open(
            &path,
            &write::OpenOptions::new(write::Mode::CreateOrTruncate)
                .with_parents(),
        )
        .expect("writer should be created");
        writer.write_all(b"abc").unwrap();
        drop(writer);
    }

    {
        let mut writer = write::open(&path, &write::OpenOptions::default())
            .expect("writer should be created");
        writer.write_all(b"xyz").unwrap();
        drop(writer);
    }

    let mut reader = read::open(&path).expect("reader should open");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    assert_eq!(b"xyz", content.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_reader_returns_open_error() {
    let dir = temp_dir("open-error");

    let error = read::open(&dir.join("missing.txt"))
        .expect_err("missing file should return open error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    let source = std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<Error>())
        .expect(
            "path context should retain the native I/O error as its source",
        );
    assert_eq!(ErrorKind::NotFound, source.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_open_reader_returns_open_error_after_metadata_success() {
    let dir = temp_dir("open-reader-permission-error");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

    let error = read::open(&path)
        .expect_err("unreadable file should return open error");

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_respects_modes_parent_creation_and_buffering_options() {
    let dir = temp_dir("open-writer-options");
    let path = dir.join("nested").join("data.txt");

    {
        let mut writer = write::open(
            &path,
            &write::OpenOptions::new(write::Mode::CreateNew).with_parents(),
        )
        .expect("create-new writer should create missing parents");
        writer.write_all(b"one").unwrap();
        drop(writer);
    }

    let error =
        write::open(&path, &write::OpenOptions::new(write::Mode::CreateNew))
            .expect_err("create-new mode should reject existing files");
    assert_eq!(ErrorKind::AlreadyExists, error.kind());

    {
        let mut writer = write::open(
            &path,
            &write::OpenOptions::new(write::Mode::AppendExisting),
        )
        .expect("append-existing writer should open existing files");
        writer.write_all(b"-two").unwrap();
        drop(writer);
    }
    assert_eq!(b"one-two", fs::read(&path).unwrap().as_slice());

    {
        let mut writer = write::open(&path, &write::OpenOptions::default())
            .expect("default writer should create or truncate");
        writer.write_all(b"three").unwrap();
        drop(writer);
    }
    assert_eq!(b"three", fs::read(&path).unwrap().as_slice());

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_reader_and_writer_cover_unbuffered_and_append_or_create_modes() {
    let dir = temp_dir("open-writer-extra-modes");
    let path = dir.join("data.txt");
    fs::write(&path, b"abcdef").unwrap();

    {
        let mut writer = write::open(
            &path,
            &write::OpenOptions::new(write::Mode::OpenExistingAtStart),
        )
        .expect("open-existing-at-start writer should open");
        writer.write_all(b"XY").unwrap();
        drop(writer);
    }
    assert_eq!(b"XYcdef", fs::read(&path).unwrap().as_slice());

    {
        let mut writer = write::open(
            &path,
            &write::OpenOptions::new(write::Mode::AppendOrCreate),
        )
        .expect("append-or-create writer should open");
        writer.write_all(b"-tail").unwrap();
        drop(writer);
    }

    let mut reader = read::open(&path).expect("reader should open");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    assert_eq!(b"XYcdef-tail", content.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_returns_open_error_for_missing_parent_without_parent_creation()
 {
    let dir = temp_dir("open-writer-missing-parent");

    let error = write::open(
        &dir.join("missing").join("data.txt"),
        &write::OpenOptions::default(),
    )
    .expect_err("missing parent should return writer open error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_exists_metadata_and_list_report_local_paths() {
    let dir = temp_dir("metadata-list");
    let path = dir.join("data.txt");
    fs::write(&path, b"abc").unwrap();

    let mut names = directory::read(&dir)
        .expect("directory should be listed")
        .map(|entry| entry.expect("entry should be readable").file_name())
        .collect::<Vec<_>>();
    names.sort();

    assert!(metadata::exists(&path).expect("existing file should be checked"));
    assert_eq!(3, metadata::read(&path).unwrap().len());
    assert_eq!(vec![std::ffi::OsString::from("data.txt")], names);
    assert!(!metadata::exists(&dir.join("missing.txt")).unwrap());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_returns_parent_error() {
    let dir = temp_dir("parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = write::open(
        &file_parent.join("child.txt"),
        &write::OpenOptions::default().with_parents(),
    )
    .expect_err("file parent should return create-dir error");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_open_writer_returns_parent_creation_error_for_dangling_symlink() {
    let dir = temp_dir("dangling-parent-error");
    let dangling_parent = dir.join("dangling-parent");
    symlink(dir.join("missing-target"), &dangling_parent)
        .expect("dangling parent symlink should be created");

    let error = write::open(
        &dangling_parent.join("child.txt"),
        &write::OpenOptions::default().with_parents(),
    )
    .expect_err("parent creation should reject a dangling symlink");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotFound
    ));
    fs::remove_dir_all(dir).expect("temporary fixture should be removed");
}
