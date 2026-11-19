// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::outcome::LocalCopyFailureState;

#[test]
fn test_observed_reader_rejects_unrepresentable_binding_deadline() {
    assert!(std::time::Instant::now().checked_add(Duration::MAX).is_none());
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    std::fs::write(fixture.path().join("item"), b"payload").expect("fixture file should be written");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let path = Path::new("item");

    let error = filesystem
        .open_observed_reader(
            path,
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::MAX)),
        )
        .expect_err("unrepresentable deadline should be rejected");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::OpenReader);
    assert_eq!(error.path(), Some(path));
    assert_eq!(
        error.reason(),
        Some("path binding deadline exceeds the monotonic clock range")
    );

    let expired = filesystem
        .open_observed_reader(
            path,
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::ZERO)),
        )
        .expect_err("zero deadline should be immediately expired");
    assert_eq!(expired.io_error_kind(), std::io::ErrorKind::TimedOut);

    filesystem
        .open_observed_reader(path, &LocalObservedReadOptions::new())
        .expect("no binding deadline remains unlimited");
    filesystem
        .open_observed_reader(
            path,
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::from_secs(5))),
        )
        .expect("a representable finite deadline is accepted");
}

#[test]
fn test_child_directory_open_rejects_unrepresentable_binding_deadline() {
    assert!(std::time::Instant::now().checked_add(Duration::MAX).is_none());
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    std::fs::create_dir(fixture.path().join("child")).expect("child directory should be created");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let root = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("root directory handle should open");
    let path = Path::new("child");

    let error = root
        .open_directory(
            path,
            &LocalDirectoryOpenOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::MAX)),
        )
        .expect_err("unrepresentable deadline should be rejected");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
    assert_eq!(error.path(), Some(path));
    assert_eq!(
        error.reason(),
        Some("path binding deadline exceeds the monotonic clock range")
    );
}

#[test]
fn test_directory_handle_acquisition_rejects_unrepresentable_binding_deadline() {
    assert!(std::time::Instant::now().checked_add(Duration::MAX).is_none());
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let path = Path::new("/");

    let error = filesystem
        .open_directory_with_options(
            path,
            &LocalDirectoryOpenOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::MAX)),
        )
        .expect_err("unrepresentable deadline should be rejected");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
    assert_eq!(error.path(), Some(path));
    assert_eq!(
        error.reason(),
        Some("path binding deadline exceeds the monotonic clock range")
    );
}

#[test]
fn test_fixed_directory_reader_accepts_finite_authority_and_reader_deadlines() {
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    std::fs::write(fixture.path().join("item"), b"payload").expect("fixture file should be written");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let directory = filesystem
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::from_secs(10))),
        )
        .expect("finite authority deadline should open");

    directory
        .open_observed_reader(
            Path::new("item"),
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::from_secs(5))),
        )
        .expect("finite reader deadline should combine with the authority deadline");
}

#[test]
fn test_directory_copy_combines_binding_and_operation_deadlines() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let source = Path::new("source");
    let destination = Path::new("destination");
    std::fs::write(fixture.path().join(source), b"payload").expect("source file");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let handle = filesystem
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::from_secs(60))),
        )
        .expect("directory handle with binding deadline");

    let failure = handle
        .copy_with_options(
            source,
            destination,
            &LocalCopyOptions::new().with_deadline(Duration::ZERO),
        )
        .expect_err("operation deadline expires before the binding deadline");
    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.error().io_error_kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(failure.error().operation(), LocalFileOperation::Copy);
    assert_eq!(failure.error().path(), Some(source));
    assert_eq!(failure.error().target(), Some(destination));
    assert!(!fixture.path().join(destination).exists());

    let escaping_destination = Path::new("../destination");
    let failure = handle
        .copy_with_options(source, escaping_destination, &LocalCopyOptions::new())
        .expect_err("destination cannot escape the directory authority");
    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.error().kind(), LocalFileErrorKind::InvalidPath);
    assert_eq!(failure.error().path(), Some(source));
    assert_eq!(failure.error().target(), Some(escaping_destination));
    assert_eq!(
        std::fs::read(fixture.path().join(source)).expect("preserved source"),
        b"payload"
    );
}
