// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalFiles,
};
use std::io::{
    Error,
    ErrorKind,
    Read,
    Write,
};

#[cfg(unix)]
use super::super::test_support::PermissionsExt;
#[cfg(windows)]
use super::super::test_support::path_with_interior_nul;
use super::super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_open_reader_and_writer_replace_old_buffered_helpers() {
    let dir = temp_dir("buffered");
    let path = dir.join("a").join("b").join("data.txt");

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
                .with_parent(),
        )
        .expect("writer should be created");
        writer.write_all(b"abc").unwrap();
        writer.close().unwrap();
    }

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::default().buffered(),
        )
        .expect("buffered writer should be created");
        writer.write_all(b"xyz").unwrap();
        writer.close().unwrap();
    }

    let mut reader =
        LocalFiles::open_reader(&path, FileReadOptions::buffered())
            .expect("reader should open");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    assert_eq!(b"xyz", content.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_reader_returns_open_error() {
    let dir = temp_dir("open-error");

    let error = LocalFiles::open_reader(
        dir.join("missing.txt"),
        FileReadOptions::default(),
    )
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

    let error = LocalFiles::open_reader(&path, FileReadOptions::default())
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
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::CreateNew)
                .with_parent()
                .buffered(),
        )
        .expect("create-new writer should create missing parents");
        assert!(writer.is_buffered());
        writer.write_all(b"one").unwrap();
        writer.close().unwrap();
    }

    let error = LocalFiles::open_writer(
        &path,
        FileWriteOptions::new(FileWriteMode::CreateNew),
    )
    .expect_err("create-new mode should reject existing files");
    assert_eq!(ErrorKind::AlreadyExists, error.kind());

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::AppendExisting),
        )
        .expect("append-existing writer should open existing files");
        writer.write_all(b"-two").unwrap();
        writer.close().unwrap();
    }
    assert_eq!(b"one-two", fs::read(&path).unwrap().as_slice());

    {
        let mut writer =
            LocalFiles::open_writer(&path, FileWriteOptions::default())
                .expect("default writer should create or truncate");
        writer.write_all(b"three").unwrap();
        writer.close().unwrap();
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
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::OpenExistingAtStart),
        )
        .expect("open-existing-at-start writer should open");
        assert!(!writer.is_buffered());
        writer.write_all(b"XY").unwrap();
        writer.close().unwrap();
    }
    assert_eq!(b"XYcdef", fs::read(&path).unwrap().as_slice());

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::AppendOrCreate)
                .buffered_with_capacity(16)
                .expect("positive buffer capacity should be accepted"),
        )
        .expect("append-or-create writer should open");
        assert!(writer.is_buffered());
        writer.write_all(b"-tail").unwrap();
        writer.close().unwrap();
    }

    let mut reader =
        LocalFiles::open_reader(&path, FileReadOptions::unbuffered())
            .expect("unbuffered reader should open");
    assert!(!reader.is_buffered());
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    assert_eq!(b"XYcdef-tail", content.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_returns_open_error_for_missing_parent_without_parent_creation()
 {
    let dir = temp_dir("open-writer-missing-parent");

    let error = LocalFiles::open_writer(
        dir.join("missing").join("data.txt"),
        FileWriteOptions::default(),
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

    let mut names = LocalFiles::list(&dir)
        .expect("directory should be listed")
        .map(|entry| entry.expect("entry should be readable").file_name())
        .collect::<Vec<_>>();
    names.sort();

    assert!(
        LocalFiles::exists(&path).expect("existing file should be checked")
    );
    assert_eq!(3, LocalFiles::metadata(&path).unwrap().len());
    assert_eq!(vec![std::ffi::OsString::from("data.txt")], names);
    assert!(!LocalFiles::exists(dir.join("missing.txt")).unwrap());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_returns_parent_error() {
    let dir = temp_dir("parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = LocalFiles::open_writer(
        file_parent.join("child.txt"),
        FileWriteOptions::default().with_parent(),
    )
    .expect_err("file parent should return create-dir error");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    fs::remove_dir_all(dir).unwrap();
}
