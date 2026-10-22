// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSliceMut;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_io::Input;
use qubit_io::InputRef;
use qubit_local_files::LocalFileSystem;

#[test]
fn reader_indexed_range_and_adapter_share_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"abcdef").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let mut reader = filesystem.open_reader(&path).unwrap();
    let mut output = [b'!'; 6];

    // SAFETY: 1..4 is a valid destination range.
    assert_eq!(
        unsafe { Input::read_unchecked(&mut reader, &mut output, 1, 3) }.unwrap(),
        3
    );
    assert_eq!(&output, b"!abc!!");
    assert!(!Input::is_buffered(&reader));

    {
        let mut std_reader = InputRef::new(&mut reader).into_std_read();
        std_reader.seek(SeekFrom::Start(4)).unwrap();
        let mut suffix = [0; 2];
        std_reader.read_exact(&mut suffix).unwrap();
        assert_eq!(&suffix, b"ef");
    }
    assert_eq!(reader.stream_position().unwrap(), 6);
}

#[test]
fn reader_handles_eof_zero_length_and_owned_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"abc").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let mut reader = filesystem.open_reader(&path).unwrap();
    let mut output = [b'!'; 4];

    // SAFETY: 4..4 is an empty valid destination range at the slice end.
    assert_eq!(
        unsafe { Input::read_unchecked(&mut reader, &mut output, 4, 0) }.unwrap(),
        0
    );
    // SAFETY: 0..4 is a valid destination range; the file only has three bytes.
    assert_eq!(
        unsafe { Input::read_unchecked(&mut reader, &mut output, 0, 4) }.unwrap(),
        3
    );
    assert_eq!(&output, b"abc!");
    assert_eq!(Input::read(&mut reader, &mut output).unwrap(), 0);

    let std_reader = reader.into_std_read();
    let mut reader = std_reader.into_inner();
    assert_eq!(reader.stream_position().unwrap(), 3);
}

#[test]
fn inherent_read_vectored_skips_empty_buffers_and_reads_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"abcdef").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let mut reader = filesystem.open_reader(&path).unwrap();
    let mut first = [0; 2];
    let mut second = [0; 4];
    let mut buffers = [
        IoSliceMut::new(&mut []),
        IoSliceMut::new(&mut first),
        IoSliceMut::new(&mut second),
    ];

    assert_eq!(reader.read_vectored(&mut buffers).unwrap(), 6);
    assert_eq!(&first, b"ab");
    assert_eq!(&second, b"cdef");
}

#[test]
fn read_prefix_keeps_limit_and_zero_limit_behavior() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"abcdef").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();

    assert_eq!(filesystem.read_prefix(&path, 3).unwrap(), b"abc");
    assert!(filesystem.read_prefix(&path, 0).unwrap().is_empty());
}
