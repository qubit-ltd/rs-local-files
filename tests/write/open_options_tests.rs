// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::io::Write;
use std::time::Duration;

use qubit_local_files::write::{
    self,
    Mode,
    OpenOptions,
};

/// Verifies default write options contain no hidden retry or parent creation.
#[test]
fn test_open_options_default_is_explicit() {
    let options = OpenOptions::default();

    assert_eq!(Mode::CreateOrTruncate, options.mode());
    assert!(!options.creates_parents());
    assert_eq!(Duration::ZERO, options.open_retry_timeout());
}

/// Verifies builders configure the native open contract.
#[test]
fn test_open_options_builders_update_native_behavior() {
    let timeout = Duration::from_millis(25);
    let options = OpenOptions::new(Mode::CreateNew)
        .with_parents()
        .with_open_retry_timeout(timeout);

    assert_eq!(Mode::CreateNew, options.mode());
    assert!(options.creates_parents());
    assert_eq!(timeout, options.open_retry_timeout());
}

/// Verifies that the native write API returns the standard file handle.
#[test]
fn test_open_returns_std_file() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should be created");
    let path = directory.path().join("nested").join("output.bin");
    let options = OpenOptions::new(Mode::CreateNew).with_parents();

    let mut file: File = write::open(&path, &options)
        .expect("the new regular file should open for writing");
    file.write_all(b"payload")
        .expect("the opened file should accept bytes");
    file.flush().expect("the opened file should flush");

    assert_eq!(
        b"payload",
        std::fs::read(path)
            .expect("the written fixture should be readable")
            .as_slice()
    );
}
