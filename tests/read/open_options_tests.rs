// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::time::Duration;

use qubit_local_files::read::{
    self,
    OpenOptions,
};

/// Verifies that read opens do not retry unless callers opt in.
#[test]
fn test_open_options_default_disables_retry() {
    let options = OpenOptions::default();

    assert_eq!(Duration::ZERO, options.open_retry_timeout());
}

/// Verifies the retry builder retains the requested timeout.
#[test]
fn test_open_options_sets_retry_timeout() {
    let options = OpenOptions::default()
        .with_open_retry_timeout(Duration::from_millis(25));

    assert_eq!(Duration::from_millis(25), options.open_retry_timeout());
}

/// Verifies that the native read API returns the standard file handle.
#[test]
fn test_open_returns_std_file() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should be created");
    let path = directory.path().join("input.bin");
    std::fs::write(&path, b"payload").expect("the fixture should be written");

    let file: File = read::open(&path, &OpenOptions::default())
        .expect("the regular file should open for reading");

    assert_eq!(
        7,
        file.metadata()
            .expect("opened file metadata should be available")
            .len()
    );
}
