// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

use qubit_local_files::LocalFileError;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;

fn error(reason: &'static str) -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::Io, LocalFileOperation::Read).with_reason(reason)
}

#[test]
fn test_public_append_retains_primary_and_nested_order() {
    let error = error("primary")
        .with_additional_cleanup_error(error("first"))
        .with_additional_cleanup_error(error("second").with_cleanup_error(error("third")));

    assert_eq!(error.reason(), Some("primary"));
    let first = error.cleanup_error().expect("first cleanup error");
    assert_eq!(first.reason(), Some("first"));
    let second = first.cleanup_error().expect("second cleanup error");
    assert_eq!(second.reason(), Some("second"));
    assert_eq!(second.cleanup_error().and_then(LocalFileError::reason), Some("third"));
}

#[test]
fn test_replacing_cleanup_error_keeps_its_existing_replacement_semantics() {
    let error = error("primary")
        .with_cleanup_error(error("old"))
        .with_cleanup_error(error("new"));

    assert_eq!(error.reason(), Some("primary"));
    assert_eq!(error.cleanup_error().and_then(LocalFileError::reason), Some("new"));
}

#[test]
fn test_additional_cleanup_error_appends_without_replacing_existing_chain() {
    let primary = LocalFileError::from_io(
        LocalFileOperation::Read,
        Some("primary".into()),
        None,
        io::Error::other("primary failure"),
    );
    let first_cleanup = LocalFileError::from_io(
        LocalFileOperation::Abort,
        Some("first cleanup".into()),
        None,
        io::Error::other("first cleanup failure"),
    );
    let second_cleanup = LocalFileError::from_io(
        LocalFileOperation::Abort,
        Some("second cleanup".into()),
        None,
        io::Error::other("second cleanup failure"),
    );

    let error = primary
        .with_cleanup_error(first_cleanup)
        .with_additional_cleanup_error(second_cleanup);

    assert_eq!(
        Some("primary failure"),
        error.io_error().map(ToString::to_string).as_deref()
    );
    let first = error.cleanup_error().expect("first cleanup remains");
    assert_eq!(
        Some("first cleanup failure"),
        first.io_error().map(ToString::to_string).as_deref()
    );
    let second = first.cleanup_error().expect("second cleanup is appended");
    assert_eq!(
        Some("second cleanup failure"),
        second.io_error().map(ToString::to_string).as_deref()
    );
}

#[cfg(feature = "test-support")]
#[test]
fn test_appended_recovery_owner_remains_extractable_and_cleanable() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::test_support::install_test_fault;
    use tempfile::tempdir;

    let parent = tempdir().expect("parent directory");
    let _fault = install_test_fault("temp-identity-file-cleanup").expect("cleanup fault");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let recovery_error = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect_err("injected cleanup failure should retain recovery ownership");
    assert!(recovery_error.temp_recovery().is_some());

    assert!(
        recovery_error.cleanup_error().is_some(),
        "fixture includes an existing cleanup chain"
    );
    let error = recovery_error.with_additional_cleanup_error(error("later cleanup"));
    assert!(error.temp_recovery().is_some(), "recovery owner remains attached");
    let first = error.cleanup_error().expect("existing cleanup error");
    assert!(first.temp_recovery().is_none(), "fixture's recovery remains on primary");
    assert_eq!(
        first.cleanup_error().and_then(LocalFileError::reason),
        Some("later cleanup"),
        "appended error follows the fixture's cleanup failure"
    );

    let (error, recovery) = error.into_temp_recovery_parts();
    assert!(error.temp_recovery().is_none(), "ownership transferred from error");
    let mut recovery = recovery.expect("extractable recovery owner");
    drop(_fault);
    recovery
        .cleanup()
        .expect("explicit cleanup after removing injected fault");
    assert_eq!(
        std::fs::read_dir(parent.path())
            .expect("parent remains readable")
            .count(),
        0
    );
}
