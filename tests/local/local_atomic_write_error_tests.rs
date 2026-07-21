// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::io::{
    Error,
    ErrorKind,
};

use qubit_local_files::{
    LocalAtomicDestinationState,
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
    assert_eq!(error.kind(), error.source_error().kind());
    let dynamic_source = StdError::source(&error)
        .and_then(|source| source.downcast_ref::<Error>())
        .expect("error source should retain the native I/O error");
    assert!(std::ptr::eq(error.source_error(), dynamic_source));
    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
    assert_eq!(file_parent.join("child.txt"), error.path());
    assert!(error.temporary_path().is_none());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert!(error.cleanup_error().is_none());
    assert!(error.parent_sync_error().is_none());
    let base_message = error.to_string();
    assert!(base_message.contains("PrepareParent"));
    assert!(!base_message.contains("staging path"));
    let display_path = dir.join("display.txt");
    let display_error = LocalFiles::atomic_write_with(&display_path, |_| {
        Err(Error::other("write failed"))
    })
    .expect_err("callback failure should return staging context");
    let temporary_path = display_error
        .temporary_path()
        .map(ToOwned::to_owned)
        .expect("callback failure should retain staging path");
    let message = display_error.to_string();
    assert!(message.contains(&temporary_path.display().to_string()));
    assert!(message.contains("destination_state=Unchanged"));
    fs::remove_dir_all(dir).unwrap();
}
