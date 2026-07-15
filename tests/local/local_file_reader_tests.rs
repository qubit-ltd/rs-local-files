// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::{
    ErrorKind,
    Read,
    Seek,
    SeekFrom,
};

use qubit_local_files::{
    FileReadOptions,
    LocalFiles,
};

use super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_open_reader_respects_buffering_options_and_rejects_directories() {
    let dir = temp_dir("open-reader-options");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").unwrap();

    let mut reader = LocalFiles::open_reader(
        &path,
        FileReadOptions::buffered_with_capacity(16)
            .expect("positive buffer capacity should be accepted"),
    )
    .expect("buffered reader should open");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    let error = LocalFiles::open_reader(&dir, FileReadOptions::default())
        .expect_err("directories should not be accepted as files");

    assert!(reader.is_buffered());
    assert_eq!(b"payload", content.as_slice());
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(
        error.to_string().contains("opened path is not a file"),
        "reader validation must describe the resource obtained from open: {error}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_file_reader_supports_seek_for_unbuffered_and_buffered_readers() {
    let dir = temp_dir("reader-seek");
    let path = dir.join("data.txt");
    fs::write(&path, b"abcdef").unwrap();

    let mut unbuffered =
        LocalFiles::open_reader(&path, FileReadOptions::unbuffered())
            .expect("unbuffered reader should open");
    let mut unbuffered_bytes = [0; 2];
    assert_eq!(
        2,
        unbuffered
            .seek(SeekFrom::Start(2))
            .expect("unbuffered reader should seek")
    );
    unbuffered
        .read_exact(&mut unbuffered_bytes)
        .expect("unbuffered reader should read from seeked position");

    let mut buffered = LocalFiles::open_reader(
        &path,
        FileReadOptions::buffered_with_capacity(4)
            .expect("positive buffer capacity should be accepted"),
    )
    .expect("buffered reader should open");
    let mut first = [0; 1];
    let mut buffered_tail = Vec::new();
    buffered
        .read_exact(&mut first)
        .expect("buffered reader should fill its internal buffer");
    assert_eq!(
        4,
        buffered
            .seek(SeekFrom::Start(4))
            .expect("buffered reader should seek")
    );
    buffered
        .read_to_end(&mut buffered_tail)
        .expect("buffered reader should read after seek");

    assert_eq!(b"cd", &unbuffered_bytes);
    assert_eq!(b"a", &first);
    assert_eq!(b"ef", buffered_tail.as_slice());
    fs::remove_dir_all(dir).unwrap();
}
