// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::time::Duration;

use crate::LocalFileSystem;
use crate::directory::LocalComponentPolicy;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;
use crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary;
use crate::tests::rooted::support::resolution_observation_tests::DeadlineClockGuard;
use crate::tests::rooted::support::resolution_observation_tests::metadata_count_at_boundary;
use crate::tests::rooted::support::resolution_observation_tests::snapshot;

fn fixture() -> (tempfile::TempDir, LocalFileSystem) {
    let scratch = tempfile::tempdir().expect("fixture directory should be created");
    std::fs::write(scratch.path().join("item"), b"payload").expect("fixture file should be created");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("rooted filesystem should open");
    (scratch, filesystem)
}

fn binding_options() -> LocalObservedReadOptions {
    LocalObservedReadOptions::new()
        .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::from_secs(60)))
}

#[test]
fn test_rlf5_observed_namespace_reader_keeps_deadline_during_content_resolution() {
    let (scratch, filesystem) = fixture();
    let _clock = DeadlineClockGuard::expire_at(DeadlineBoundary::ObservedReaderBeforeContentResolution);

    let error = filesystem
        .open_observed_reader(Path::new("item"), &binding_options())
        .expect_err("actual content resolution must retain the binding deadline");

    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
    assert_eq!(Some(Path::new("/item")), error.path());
    assert_eq!(Some(Path::new("/")), error.current_directory());
    assert_eq!(Some(snapshot().0), metadata_count_at_boundary());
    assert_eq!(
        b"payload",
        std::fs::read(scratch.path().join("item")).unwrap().as_slice()
    );
}

#[test]
fn test_rlf5_observed_fixed_directory_reader_keeps_deadline_during_content_resolution() {
    let (scratch, filesystem) = fixture();
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    let _clock = DeadlineClockGuard::expire_at(DeadlineBoundary::ObservedReaderBeforeContentResolution);

    let error = directory
        .open_observed_reader(Path::new("item"), &binding_options())
        .expect_err("fixed content resolution must retain the binding deadline");

    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
    let expected_path = scratch.path().join("item");
    assert_eq!(Some(expected_path.as_path()), error.path());
    assert_eq!(Some(snapshot().0), metadata_count_at_boundary());
    assert_eq!(
        b"payload",
        std::fs::read(scratch.path().join("item")).unwrap().as_slice()
    );
}

#[test]
fn test_rlf5_observed_binding_verification_propagates_reopen_timeout() {
    let (_scratch, filesystem) = fixture();
    let reader = filesystem
        .open_observed_reader(Path::new("item"), &binding_options())
        .expect("initial observed reader should open before expiration");
    let _clock = DeadlineClockGuard::expire_at(DeadlineBoundary::ObservedReaderBeforeContentResolution);

    let error = reader
        .verify_path_binding()
        .expect_err("binding verification must propagate the reopened reader timeout");

    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
    assert_eq!(Some(Path::new("/item")), error.path());
    assert_eq!(Some(Path::new("/")), error.current_directory());
}

#[test]
fn test_rlf5_observed_reader_without_binding_deadline_still_reads() {
    let (_scratch, filesystem) = fixture();
    for options in [
        LocalObservedReadOptions::new(),
        LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        binding_options(),
    ] {
        let mut reader = filesystem
            .open_observed_reader(Path::new("item"), &options)
            .expect("reader should open without an injected expiration");
        let contents = reader
            .read_to_end_limited(32)
            .expect("fixture contents should be readable");
        assert_eq!(b"payload", contents.as_slice());
    }
}

#[test]
fn test_fixed_reader_rejects_overflow_when_authority_and_reader_deadlines_are_maximum() {
    assert!(std::time::Instant::now().checked_add(Duration::MAX).is_none());
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    std::fs::write(fixture.path().join("item"), b"payload").expect("fixture file should be written");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let path = Path::new("item");
    let options =
        LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::MAX));

    let error = filesystem
        .open_observed_reader_fixed(
            path,
            path,
            &LocalComponentPolicy::allow_all(),
            &options,
            Some(Duration::MAX),
        )
        .expect_err("combined unrepresentable deadlines should be rejected");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::OpenReader);
    assert_eq!(error.path(), Some(path));
    assert_eq!(
        error.reason(),
        Some("path binding deadline exceeds the monotonic clock range")
    );
}

#[test]
fn test_fixed_reader_uses_the_smaller_authority_or_reader_deadline() {
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    std::fs::write(fixture.path().join("item"), b"payload").expect("fixture file should be written");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let path = Path::new("item");
    let open_fixed = |reader_deadline, authority_deadline| {
        let options = LocalObservedReadOptions::new()
            .with_path_binding(LocalPathBindingOptions::new().with_deadline(reader_deadline));
        filesystem.open_observed_reader_fixed(
            path,
            path,
            &LocalComponentPolicy::allow_all(),
            &options,
            Some(authority_deadline),
        )
    };

    for (reader_deadline, authority_deadline) in [
        (Duration::ZERO, Duration::from_secs(5)),
        (Duration::from_secs(5), Duration::ZERO),
    ] {
        let error = open_fixed(reader_deadline, authority_deadline)
            .expect_err("the smaller zero deadline should expire immediately");
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    }
}
