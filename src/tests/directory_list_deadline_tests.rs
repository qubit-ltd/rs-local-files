// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::directory::LocalDirectoryHandle;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalPathBindingOptions;
use crate::tests::rooted::support::resolution_observation_tests;

fn handle_with_child(binding: Option<Duration>) -> (tempfile::TempDir, LocalDirectoryHandle) {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir_all(fixture.path().join("child/nested")).expect("child tree should exist");
    fs::write(fixture.path().join("child/nested/item"), b"item").expect("item should exist");
    let mut options = LocalDirectoryOpenOptions::new();
    if let Some(deadline) = binding {
        options = options.with_path_binding(LocalPathBindingOptions::new().with_deadline(deadline));
    }
    let handle = LocalFileSystem::rooted(fixture.path())
        .expect("rooted filesystem should open")
        .open_directory_with_options(Path::new("/"), &options)
        .expect("directory handle should open before arming the clock");
    (fixture, handle)
}

#[test]
fn test_list_deadline_rejects_before_path_prevalidation() {
    let (_fixture, handle) = handle_with_child(None);
    resolution_observation_tests::reset();
    let _clock = resolution_observation_tests::DeadlineClockGuard::expire_after(0);

    let error = handle
        .list_with_options(
            Path::new("child/nested"),
            &LocalListOptions::new().with_deadline(Duration::from_secs(60)),
        )
        .expect_err("expired prevalidation must stop listing");

    assert_eq!(error.operation(), LocalFileOperation::List);
    assert_eq!(error.kind(), LocalFileErrorKind::Io);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    let (metadata, directory_open, _) = resolution_observation_tests::snapshot();
    assert_eq!(metadata, 0);
    assert_eq!(directory_open, 0);
}

#[test]
fn test_list_started_at_does_not_restart_an_expired_budget() {
    let (_fixture, handle) = handle_with_child(None);
    let started_at = Instant::now() - Duration::from_secs(60);

    let error = handle
        .list_with_options_started_at(
            Path::new("child"),
            &LocalListOptions::new().with_deadline(Duration::from_secs(30)),
            started_at,
        )
        .expect_err("an expired start must remain expired");

    assert_eq!(error.operation(), LocalFileOperation::List);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
}

#[test]
fn test_list_deadline_overflow_is_invalid_options_before_resolution() {
    let (_fixture, handle) = handle_with_child(None);
    resolution_observation_tests::reset();

    let error = handle
        .list_with_options(
            Path::new("child"),
            &LocalListOptions::new().with_deadline(Duration::MAX),
        )
        .expect_err("unrepresentable deadline must be rejected");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::List);
    let (metadata, directory_open, _) = resolution_observation_tests::snapshot();
    assert_eq!(metadata, 0);
    assert_eq!(directory_open, 0);
}

#[test]
fn test_list_uses_earlier_operation_or_binding_deadline() {
    for (operation, binding) in [
        (Duration::from_secs(20), Duration::from_secs(60)),
        (Duration::from_secs(60), Duration::from_secs(20)),
    ] {
        let (_fixture, handle) = handle_with_child(Some(binding));
        resolution_observation_tests::reset();
        let started_at = Instant::now() - Duration::from_secs(30);

        let error = handle
            .list_with_options_started_at(
                Path::new("child"),
                &LocalListOptions::new().with_deadline(operation),
                started_at,
            )
            .expect_err("the earlier deadline must expire listing");

        assert_eq!(error.operation(), LocalFileOperation::List);
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
        let (metadata, directory_open, _) = resolution_observation_tests::snapshot();
        assert_eq!(metadata, 0);
        assert_eq!(directory_open, 0);
    }
}

#[test]
fn test_list_deadline_expiring_during_resolution_stops_additional_io() {
    let (_fixture, handle) = handle_with_child(None);
    resolution_observation_tests::reset();
    let _clock = resolution_observation_tests::DeadlineClockGuard::expire_after(2);

    let error = handle
        .list_with_options(
            Path::new("child/nested"),
            &LocalListOptions::new().with_deadline(Duration::from_secs(60)),
        )
        .expect_err("deadline should expire during resolution");

    assert_eq!(error.operation(), LocalFileOperation::List);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    let (metadata, directory_open, _) = resolution_observation_tests::snapshot();
    assert!(metadata <= 1, "resolution must stop metadata work after expiry");
    assert_eq!(directory_open, 0);
}

#[test]
fn test_list_without_deadline_succeeds() {
    let (_fixture, handle) = handle_with_child(None);

    let walker = handle
        .list_with_options(Path::new("child"), &LocalListOptions::new())
        .expect("listing without a deadline should succeed");
    assert!(walker.root().ends_with("child"));
    assert_eq!(walker.count(), 1);
}

#[cfg(feature = "test-support")]
#[test]
fn test_list_entry_read_failure_is_returned_by_the_walker() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("entry"), b"payload").expect("entry should exist");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut walker = filesystem
        .list_with_options(fixture.path(), &LocalListOptions::new())
        .expect("walker should open before the child read fault");
    let _fault = crate::local::install_test_fault("walker-entry").expect("entry-read fault should install");

    let error = walker
        .next()
        .expect("fault must be yielded as a walker item")
        .expect_err("entry read should fail");

    assert_eq!(LocalFileOperation::List, error.operation());
    assert_eq!(LocalFileErrorKind::Io, error.kind());
    assert_eq!(std::io::ErrorKind::Other, error.io_error_kind());
}
