// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Behavioral coverage for synchronous local file readers.

use std::io::{IoSliceMut, Read, Seek, SeekFrom};

use qubit_local_files::{LocalFileSystem, LocalReadOptions};
use tempfile::tempdir;

/// Verifies readers expose the native handle and support sequential seeking.
#[test]
fn test_local_file_reader_reads_and_seeks() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abcdef").expect("fixture should be written");

    let mut reader = LocalFileSystem::open_reader(&path, &LocalReadOptions::new())
        .expect("regular file should open for reading");
    assert!(
        reader
            .as_file()
            .metadata()
            .expect("handle metadata should load")
            .is_file()
    );

    let mut first = [0_u8; 2];
    let count = reader.read(&mut first).expect("prefix should be readable");
    assert_eq!(count, first.len());
    assert_eq!(&first, b"ab");

    reader
        .seek(SeekFrom::Start(3))
        .expect("seek should succeed");
    let mut last = [0_u8; 3];
    reader
        .read_exact(&mut last)
        .expect("suffix should be readable");
    assert_eq!(&last, b"def");
}

/// Verifies vectored reads consume bytes across mutable buffers in order.
#[test]
fn test_local_file_reader_supports_vectored_reads() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abcdef").expect("fixture should be written");

    let mut reader = LocalFileSystem::open_reader(&path, &LocalReadOptions::new())
        .expect("regular file should open for reading");
    let mut first = [0_u8; 2];
    let mut second = [0_u8; 4];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count = reader
        .read_vectored(&mut buffers)
        .expect("vectored read should succeed");

    assert_eq!(count, 6);
    assert_eq!(&first, b"ab");
    assert_eq!(&second, b"cdef");
}
