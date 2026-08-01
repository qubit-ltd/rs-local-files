// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::io::ErrorKind;

use super::api_tests::{LocalCopyDirOptions, LocalCopyDirStage};
#[cfg(all(coverage, target_os = "linux"))]
use super::test_support::run_in_coverage_fault_process;

use super::test_support::{fs, temp_dir};

#[test]
fn test_copy_dir_all_with_returns_missing_source_error() {
    let dir = temp_dir("copy-dir-missing-source");
    let missing = dir.join("missing");
    let destination = dir.join("dst");

    let error =
        qubit_local_files::copy::directory(&missing, &destination, LocalCopyDirOptions::default())
            .expect_err("missing source should return metadata error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    assert_eq!(ErrorKind::NotFound, error.error().kind());
    assert_eq!(LocalCopyDirStage::InspectSource, error.stage());
    assert_eq!(missing, error.source_path());
    assert_eq!(dir.join("dst"), error.destination_path());
    assert_eq!(0, error.stats().files());
    assert!(error.temporary_path().is_none());
    assert!(error.cleanup_error().is_none());
    assert!(error.to_string().contains("failed to copy"));
    assert!(StdError::source(&error).is_some());
    fs::remove_dir_all(dir).unwrap();
}

/// Verifies staging-copy cleanup failures retain every diagnostic field on the
/// public recursive-copy error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_copy_dir_error_reports_staging_cleanup_context() {
    const TEST_NAME: &str = concat!(
        "local::local_copy_dir_error_tests::",
        "test_copy_dir_error_reports_staging_cleanup_context",
    );
    let Some(()) = run_in_coverage_fault_process(TEST_NAME, "copy-staging-copy-cleanup", || {
        let dir = temp_dir("copy-dir-error-staging-cleanup");
        let source = dir.join("source");
        let destination = dir.join("destination");
        fs::create_dir(&source).expect("source directory should be created");
        fs::write(source.join("payload"), b"payload").expect("source payload should be written");

        let error = qubit_local_files::copy::directory(
            &source,
            &destination,
            LocalCopyDirOptions::default(),
        )
        .expect_err("injected staging copy and cleanup should fail");

        assert_eq!(LocalCopyDirStage::CopyFileContents, error.stage());
        assert_eq!(source.join("payload"), error.source_path());
        assert_eq!(destination.join("payload"), error.destination_path());
        assert_eq!(0, error.stats().files());
        let staging_path = error
            .temporary_path()
            .expect("failed staging cleanup should retain its path");
        assert!(staging_path.starts_with(&destination));
        assert_eq!(
            Some(libc::EIO),
            error.cleanup_error().and_then(std::io::Error::raw_os_error),
        );
        assert_eq!(
            std::io::Error::from_raw_os_error(libc::EIO).kind(),
            error.error().kind(),
        );
        let message = error.to_string();
        assert!(message.contains("staging path"));
        assert!(message.contains("staging cleanup also failed"));
        let source_error =
            StdError::source(&error).expect("copy error should retain its primary source");
        assert!(source_error.to_string().contains("Input/output error"));

        fs::remove_dir_all(dir).expect("test directory should be removed after cleanup");
    }) else {
        return;
    };
}
