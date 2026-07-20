// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::fs;
use std::io::ErrorKind;

use qubit_local_files::{
    LocalPersistStage,
    LocalTempFile,
};

use super::test_support::temp_dir;

#[test]
fn test_persist_error_into_parts_returns_error_and_resource() {
    let dir = temp_dir("persist-error-parts");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), Some(".tmp"), 4)
        .expect("temporary file should be created");
    let source = file.path().to_path_buf();
    let target = dir.join("target.txt");
    fs::write(&target, b"existing").expect("target fixture should be written");

    let mut persist_error = file
        .persist(&target)
        .expect_err("existing target should reject persistence");
    assert!(
        persist_error
            .to_string()
            .contains("failed to persist temporary resource")
    );
    assert!(StdError::source(&persist_error).is_some());
    assert_eq!(ErrorKind::AlreadyExists, persist_error.error().kind());
    assert_eq!(source, persist_error.resource().path());
    assert_eq!(source, persist_error.resource_mut().path());
    assert_eq!(
        ErrorKind::NotFound,
        persist_error
            .resource()
            .as_file()
            .expect_err("persist failure should retain a closed file guard")
            .kind(),
    );
    assert_eq!(target, persist_error.requested_target());
    assert_eq!(Some(target.as_path()), persist_error.resolved_target());
    assert_eq!(LocalPersistStage::InstallDestination, persist_error.stage());
    let (error, resource, requested_target, resolved_target, stage) =
        persist_error.into_parts();

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(source, resource.path());
    assert_eq!(target, requested_target);
    assert_eq!(Some(target), resolved_target);
    assert_eq!(LocalPersistStage::InstallDestination, stage);
    drop(resource);
    assert!(!source.exists());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}
