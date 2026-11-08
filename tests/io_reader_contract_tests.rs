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
fn test_reader_indexed_range_and_adapter_share_cursor() {
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
fn test_reader_handles_eof_zero_length_and_owned_adapter() {
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
fn test_inherent_read_vectored_skips_empty_buffers_and_reads_in_order() {
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
fn test_trait_vectored_read_preserves_nonzero_file_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"012345").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let mut reader = filesystem.open_reader(&path).unwrap();
    reader.seek(SeekFrom::Start(1)).unwrap();
    let mut first = [0; 2];
    let mut second = [0; 2];

    assert_eq!(
        Input::read_vectored(&mut reader, &mut [&mut first, &mut second]).unwrap(),
        4
    );
    assert_eq!(&first, b"12");
    assert_eq!(&second, b"34");
    assert_eq!(reader.stream_position().unwrap(), 5);
    let mut suffix = [0; 1];
    assert_eq!(Input::read(&mut reader, &mut suffix).unwrap(), 1);
    assert_eq!(&suffix, b"5");
}

#[test]
fn test_inherent_vectored_read_preserves_nonzero_file_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"012345").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let mut reader = filesystem.open_reader(&path).unwrap();
    reader.seek(SeekFrom::Start(1)).unwrap();
    let mut first = [0; 2];
    let mut second = [0; 2];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    assert_eq!(reader.read_vectored(&mut buffers).unwrap(), 4);
    assert_eq!(&first, b"12");
    assert_eq!(&second, b"34");
    assert_eq!(reader.stream_position().unwrap(), 5);
    let mut suffix = [0; 1];
    assert_eq!(Input::read(&mut reader, &mut suffix).unwrap(), 1);
    assert_eq!(&suffix, b"5");
}

#[test]
fn test_inherent_single_buffer_read_handles_offset_eof_and_empty_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"012345").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let mut reader = filesystem.open_reader(&path).unwrap();
    reader.seek(SeekFrom::Start(3)).unwrap();

    let mut output = [b'!'; 3];
    let mut buffers = [IoSliceMut::new(&mut output)];
    assert_eq!(reader.read_vectored(&mut buffers).unwrap(), 3);
    assert_eq!(&output, b"345");

    let mut eof = [0; 1];
    let mut buffers = [IoSliceMut::new(&mut eof)];
    assert_eq!(reader.read_vectored(&mut buffers).unwrap(), 0);

    let mut empty = [IoSliceMut::new(&mut [])];
    assert_eq!(reader.read_vectored(&mut empty).unwrap(), 0);
    assert_eq!(reader.read_vectored(&mut []).unwrap(), 0);
}

#[test]
fn test_read_prefix_keeps_limit_and_zero_limit_behavior() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"abcdef").unwrap();
    let filesystem = LocalFileSystem::host().unwrap();

    assert_eq!(filesystem.read_prefix(&path, 3).unwrap(), b"abc");
    assert!(filesystem.read_prefix(&path, 0).unwrap().is_empty());
}
