// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Behavioral coverage for synchronous local file readers.

use std::io::IoSliceMut;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalReadOptions;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::install_test_fault;
use tempfile::tempdir;

/// Verifies readers expose the native handle and support sequential seeking.
#[test]
fn test_local_file_reader_reads_and_seeks() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abcdef").expect("fixture should be written");

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&path, &LocalReadOptions::new())
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

    reader.seek(SeekFrom::Start(3)).expect("seek should succeed");
    let mut last = [0_u8; 3];
    reader.read_exact(&mut last).expect("suffix should be readable");
    assert_eq!(&last, b"def");
}

/// Verifies vectored reads consume bytes across mutable buffers in order.
#[test]
fn test_local_file_reader_supports_vectored_reads() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abcdef").expect("fixture should be written");

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&path, &LocalReadOptions::new())
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

/// Verifies a vectored read retains bytes read before a later native error.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_local_file_reader_vectored_read_retains_prior_bytes_after_later_error() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abcdef").expect("fixture should be written");

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&path, &LocalReadOptions::new())
        .expect("regular file should open for reading");
    let _fault = install_test_fault("local-file-reader-vectored-read-after-first").expect("test fault should install");
    let mut first = [0_u8; 2];
    let mut second = [0_u8; 4];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count = reader
        .read_vectored(&mut buffers)
        .expect("bytes read before the later error should be retained");

    assert_eq!(first.len(), count);
    assert_eq!(&first, b"ab");
    assert_eq!(&second, &[0_u8; 4]);
}

/// Verifies Windows Host readers follow final links under the default policy.
#[cfg(windows)]
#[test]
fn test_local_file_reader_follows_final_file_symlink_on_windows() {
    use std::io::ErrorKind;
    use std::os::windows::fs::symlink_file;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    std::fs::write(&target, b"payload").expect("target should be written");
    if let Err(error) = symlink_file(&target, &link) {
        if error.kind() == ErrorKind::PermissionDenied {
            return;
        }
        panic!("file symlink should be created: {error}");
    }

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&link, &LocalReadOptions::new())
        .expect("the default Host policy should follow a final file symlink");
    let mut payload = Vec::new();
    reader
        .read_to_end(&mut payload)
        .expect("linked file contents should be readable");
    assert_eq!(b"payload", payload.as_slice());
}
