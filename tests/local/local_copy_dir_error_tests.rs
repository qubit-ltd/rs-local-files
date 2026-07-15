// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::io::ErrorKind;

use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFiles,
};

use super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_copy_dir_all_with_returns_missing_source_error() {
    let dir = temp_dir("copy-dir-missing-source");
    let missing = dir.join("missing");

    let error = LocalFiles::copy_dir_all_with(
        &missing,
        dir.join("dst"),
        LocalCopyDirOptions::default(),
    )
    .expect_err("missing source should return metadata error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    assert!(error.to_string().contains("failed to copy"));
    assert!(StdError::source(&error).is_some());
    fs::remove_dir_all(dir).unwrap();
}
