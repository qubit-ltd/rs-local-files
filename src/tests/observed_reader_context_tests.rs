// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[test]
fn observed_reader_observation_failure_keeps_namespace_and_pwd_context() {
    let cwd = std::env::current_dir().expect("process current directory");
    let host_fixture = tempfile::Builder::new()
        .prefix("observed-reader-context-")
        .tempdir_in(&cwd)
        .expect("host fixture under current directory");
    fs::write(host_fixture.path().join("item"), b"payload").expect("host payload");
    let host_absolute = LocalFileSystem::host().expect("host filesystem");
    let absolute_path = fs::canonicalize(host_fixture.path().join("item")).expect("absolute item path");

    let host_relative_path = host_fixture
        .path()
        .strip_prefix(&cwd)
        .expect("fixture is under the current directory")
        .join("item");
    let host_relative = LocalFileSystem::host().expect("host filesystem");

    let rooted_fixture = tempfile::tempdir().expect("rooted fixture");
    fs::create_dir(rooted_fixture.path().join("work")).expect("rooted work directory");
    fs::write(rooted_fixture.path().join("work/item"), b"payload").expect("rooted payload");
    let mut rooted = LocalFileSystem::rooted(rooted_fixture.path()).expect("rooted filesystem");
    rooted
        .set_current_directory(Path::new("/work"))
        .expect("set rooted working directory");

    let cases = [
        (&host_absolute, absolute_path.as_path(), absolute_path.as_path(), None),
        (
            &host_relative,
            host_relative_path.as_path(),
            absolute_path.as_path(),
            Some(cwd.as_path()),
        ),
        (
            &rooted,
            Path::new("item"),
            Path::new("/work/item"),
            Some(Path::new("/work")),
        ),
    ];

    for (filesystem, request_path, expected_path, expected_pwd) in cases {
        let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 1);
        let error = filesystem
            .open_observed_reader(request_path, &LocalObservedReadOptions::new())
            .expect_err("injected observation failure");
        assert!(fault.fired(), "native observation fault must be consumed");
        assert_eq!(error.operation(), LocalFileOperation::OpenReader);
        assert_eq!(error.path(), Some(expected_path));
        assert_eq!(error.current_directory(), expected_pwd);
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
        assert!(error.to_string().contains("injected native I/O contract failure"));
    }

    let error = rooted
        .open_observed_reader(
            Path::new("item"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_ancestors(0)),
        )
        .expect_err("binding ancestor limit should be reported");
    assert_eq!(error.operation(), LocalFileOperation::OpenReader);
    assert_eq!(error.path(), Some(Path::new("/work/item")));
    assert_eq!(error.current_directory(), Some(Path::new("/work")));
}

#[test]
fn fixed_observed_reader_open_failure_keeps_handle_namespace_context() {
    let fixture = tempfile::tempdir().expect("rooted fixture");
    fs::create_dir(fixture.path().join("work")).expect("work directory");
    fs::write(fixture.path().join("work/item"), b"payload").expect("payload");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let directory = filesystem
        .open_directory_with_options(Path::new("/work"), &LocalDirectoryOpenOptions::new())
        .expect("fixed directory handle");

    let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 1);
    let error = directory
        .open_observed_reader(Path::new("item"), &LocalObservedReadOptions::new())
        .expect_err("fixed authority clone should fail");

    assert!(fault.fired(), "fixed read must attempt to clone its authority");
    assert_eq!(error.operation(), LocalFileOperation::OpenReader);
    assert_eq!(error.path(), Some(fixture.path().join("work/item").as_path()));
    assert_eq!(error.current_directory(), None);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
}

#[test]
fn test_observed_reader_post_open_failures_keep_context_and_source() {
    let cwd = std::env::current_dir().expect("process current directory");
    let host_fixture = tempfile::Builder::new()
        .prefix("observed-reader-post-open-")
        .tempdir_in(&cwd)
        .expect("host fixture under current directory");
    fs::write(host_fixture.path().join("item"), b"payload").expect("host payload");
    let host = LocalFileSystem::host().expect("Host filesystem");
    let absolute_path = fs::canonicalize(host_fixture.path().join("item")).expect("absolute item path");
    let relative_path = host_fixture
        .path()
        .strip_prefix(&cwd)
        .expect("relative fixture")
        .join("item");

    let rooted_fixture = tempfile::tempdir().expect("Rooted fixture");
    fs::create_dir(rooted_fixture.path().join("work")).expect("Rooted work directory");
    fs::write(rooted_fixture.path().join("work/item"), b"payload").expect("Rooted payload");
    let mut rooted = LocalFileSystem::rooted(rooted_fixture.path()).expect("Rooted filesystem");
    rooted
        .set_current_directory(Path::new("/work"))
        .expect("set Rooted PWD");
    let directory = rooted
        .open_directory_with_options(Path::new("/work"), &LocalDirectoryOpenOptions::new())
        .expect("fixed directory");

    let cases = [
        (&host, absolute_path.as_path(), absolute_path.as_path(), None),
        (
            &host,
            relative_path.as_path(),
            absolute_path.as_path(),
            Some(cwd.as_path()),
        ),
        (
            &rooted,
            Path::new("item"),
            Path::new("/work/item"),
            Some(Path::new("/work")),
        ),
    ];
    let mut readers = Vec::new();
    for (filesystem, request_path, expected_path, expected_pwd) in cases {
        let read_reader = filesystem
            .open_observed_reader(request_path, &LocalObservedReadOptions::new())
            .expect("open reader for read failure");
        let observation_reader = filesystem
            .open_observed_reader(request_path, &LocalObservedReadOptions::new())
            .expect("open reader for observation failure");
        readers.push((
            read_reader,
            expected_path.to_path_buf(),
            expected_pwd.map(Path::to_path_buf),
        ));
        readers.push((
            observation_reader,
            expected_path.to_path_buf(),
            expected_pwd.map(Path::to_path_buf),
        ));
    }
    let fixed_expected = rooted_fixture.path().join("work/item");
    let fixed_read = directory
        .open_observed_reader(Path::new("item"), &LocalObservedReadOptions::new())
        .expect("fixed reader for read failure");
    let fixed_observation = directory
        .open_observed_reader(Path::new("item"), &LocalObservedReadOptions::new())
        .expect("fixed reader for observation failure");
    readers.push((fixed_read, fixed_expected.clone(), None));
    readers.push((fixed_observation, fixed_expected, None));

    for (index, (mut reader, expected_path, expected_pwd)) in readers.into_iter().enumerate() {
        let fault_point = if index % 2 == 0 {
            NativeIoPoint::Read
        } else {
            NativeIoPoint::Observation
        };
        let fault = NativeIoFaultGuard::fail_on(fault_point, 1);
        let error = if fault_point == NativeIoPoint::Read {
            reader.read_to_end_limited(64).expect_err("injected read failure")
        } else {
            reader.observe_now().expect_err("injected observation failure")
        };
        assert!(fault.fired(), "native fault must be consumed");
        assert_eq!(
            error.operation(),
            if fault_point == NativeIoPoint::Read {
                LocalFileOperation::Read
            } else {
                LocalFileOperation::Metadata
            }
        );
        assert_eq!(error.path(), Some(expected_path.as_path()));
        assert_eq!(error.current_directory(), expected_pwd.as_deref());
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
        assert!(error.to_string().contains("injected native I/O contract failure"));
        assert!(matches!(
            error.typed_source(),
            Some(crate::error::LocalFileErrorSource::Io(_))
        ));
    }
}
