// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;
use std::num::NonZeroUsize;

use qubit_local_files::{
    FileBuffering,
    FileReadOptions,
};

use super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_file_read_option_constructors_are_explicit() {
    assert_eq!(
        FileReadOptions {
            buffering: FileBuffering::Unbuffered,
        },
        FileReadOptions::unbuffered()
    );
    assert_eq!(
        FileReadOptions {
            buffering: FileBuffering::Buffered {
                capacity: NonZeroUsize::new(32),
            },
        },
        FileReadOptions::buffered_with_capacity(32)
            .expect("positive reader capacity should be accepted")
    );
}

#[test]
fn test_file_read_options_reject_zero_capacity() {
    let error = FileReadOptions::buffered_with_capacity(0)
        .expect_err("zero reader capacity should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
}

#[test]
fn test_reader_options_reject_zero_buffer_capacity_without_touching_file() {
    let dir = temp_dir("open-reader-zero-capacity");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").unwrap();

    let error = FileReadOptions::buffered_with_capacity(0)
        .expect_err("zero-capacity reader buffer should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(b"payload", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}
