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
    FileWriteMode,
    FileWriteOptions,
};

use super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_file_write_option_constructors_are_explicit() {
    assert_eq!(
        FileWriteOptions {
            create_parent: true,
            mode: FileWriteMode::AppendOrCreate,
            buffering: FileBuffering::Buffered {
                capacity: NonZeroUsize::new(64),
            },
        },
        FileWriteOptions::new(FileWriteMode::AppendOrCreate)
            .with_parent()
            .buffered_with_capacity(64)
            .expect("positive writer capacity should be accepted")
    );
    assert_eq!(
        FileWriteOptions {
            create_parent: false,
            mode: FileWriteMode::CreateOrTruncate,
            buffering: FileBuffering::Buffered { capacity: None },
        },
        FileWriteOptions::new(FileWriteMode::CreateOrTruncate).buffered()
    );
}

#[test]
fn test_file_write_options_reject_zero_capacity() {
    let error = FileWriteOptions::default()
        .buffered_with_capacity(0)
        .expect_err("zero writer capacity should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
}

#[test]
fn test_writer_options_reject_zero_buffer_capacity_without_mutating_target() {
    let dir = temp_dir("open-writer-zero-capacity");
    let path = dir.join("data.txt");
    fs::write(&path, b"original").expect("fixture should be written");

    let error = FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
        .buffered_with_capacity(0)
        .expect_err("zero-capacity writer buffer should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(
        b"original",
        fs::read(&path)
            .expect("target should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_writer_options_reject_zero_buffer_capacity_without_creating_parents() {
    let dir = temp_dir("open-writer-zero-capacity-parent");
    let parent = dir.join("missing").join("nested");

    let error = FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
        .with_parent()
        .buffered_with_capacity(0)
        .expect_err("zero-capacity writer buffer should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(!parent.exists());
    fs::remove_dir_all(dir).unwrap();
}
