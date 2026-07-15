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
    LocalAtomicWriteStage,
    LocalFiles,
};

use super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_atomic_write_with_returns_parent_error() {
    let dir = temp_dir("atomic-parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = LocalFiles::atomic_write_with(
        file_parent.join("child.txt"),
        |_| Ok(()),
    )
    .expect_err("file parent should return create-dir error");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage);
    assert!(!error.committed);
    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
    assert_eq!(file_parent.join("child.txt"), error.path());
    assert!(error.temporary_path().is_none());
    assert!(!error.is_committed());
    assert!(error.cleanup_error().is_none());
    assert!(StdError::source(&error).is_some());
    fs::remove_dir_all(dir).unwrap();
}
