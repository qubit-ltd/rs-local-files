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

use super::api_tests::LocalPersistStage;

use super::test_support::temp_dir;
use qubit_local_files::{LocalTempFileOptions, RootedLocalFileSystem};

#[test]
fn test_persist_error_into_parts_returns_error_and_resource() {
    let dir = temp_dir("persist-error-parts");
    let file = qubit_local_files::LocalFileSystem::create_temp_file(
        &qubit_local_files::LocalTempFileOptions::new().with_parent(&dir),
    )
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
    assert_eq!(LocalFileErrorKind::AlreadyExists, persist_error.kind());
    assert_eq!(source, persist_error.resource().path());
    assert_eq!(source, persist_error.resource_mut().path());
    assert_eq!(
        ErrorKind::NotFound,
        persist_error
            .resource_mut()
            .as_file_mut()
            .expect_err("persist failure should retain a closed file guard")
            .kind(),
    );
    assert_eq!(target, persist_error.requested_target());
    assert_eq!(Some(target.as_path()), persist_error.resolved_target());
    assert_eq!(LocalPersistStage::InstallDestination, persist_error.stage());
    let (error, resource, requested_target, resolved_target, stage, state) =
        persist_error.into_parts_with_state();

    assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
    assert_eq!(source, resource.path());
    assert_eq!(target, requested_target);
    assert_eq!(Some(target), resolved_target);
    assert_eq!(LocalPersistStage::InstallDestination, stage);
    assert_eq!(
        qubit_local_files::LocalPersistFailureState::NotPublished,
        state
    );
    drop(resource);
    assert!(!source.exists());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies target-resolution failures retain only the caller-supplied target
/// and format without an unavailable resolved path.
#[test]
fn test_persist_error_without_resolved_target_retains_context() {
    let dir = temp_dir("persist-error-unresolved-target");
    let rooted = RootedLocalFileSystem::open(&dir).expect("root authority should open");
    let file = rooted
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("rooted temporary file should be created");
    let requested = dir.join("host-absolute-target");

    let mut error = file
        .persist(&requested)
        .expect_err("rooted persistence must reject host-absolute targets");
    assert_eq!(LocalFileErrorKind::InvalidInput, error.kind());
    assert_eq!(requested, error.requested_target());
    assert_eq!(None, error.resolved_target());
    assert!(error.to_string().contains("requested target"));
    assert!(!error.to_string().contains("resolved as"));

    error
        .resource_mut()
        .cleanup()
        .expect("unresolved persistence failure should retain cleanup authority");
    let (_io, resource, _requested, resolved, _stage) = error.into_parts();
    assert_eq!(None, resolved);
    drop(resource);
    fs::remove_dir_all(dir).expect("test directory should be removed");
}
