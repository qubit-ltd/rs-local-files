// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;

#[test]
fn test_observed_reader_errors_keep_opening_context() {
    let fixture = tempfile::tempdir().expect("create fixture");
    std::fs::create_dir(fixture.path().join("work")).expect("create work");
    std::fs::create_dir(fixture.path().join("other")).expect("create other");
    std::fs::write(fixture.path().join("work/item"), b"payload").expect("write item");
    let host = LocalFileSystem::host().expect("open Host");
    let mut rooted = LocalFileSystem::rooted(fixture.path()).expect("open Rooted");
    rooted
        .set_current_directory(Path::new("/work"))
        .expect("set virtual PWD");
    let options = LocalObservedReadOptions::new();
    let native_path = fixture.path().join("work/item");
    let mut host_reader = host.open_observed_reader(&native_path, &options).expect("Host reader");
    let mut rooted_reader = rooted
        .open_observed_reader(Path::new("item"), &options)
        .expect("Rooted reader");
    let mut rooted_absolute_reader = rooted
        .open_observed_reader(Path::new("/work/item"), &options)
        .expect("Rooted absolute reader");
    let directory = rooted
        .open_directory_with_options(Path::new("/work"), &LocalDirectoryOpenOptions::new())
        .expect("fixed directory");
    let mut fixed_reader = directory
        .open_observed_reader(Path::new("item"), &options)
        .expect("fixed reader");
    rooted
        .set_current_directory(Path::new("/other"))
        .expect("change virtual PWD");
    for (reader, expected_path, expected_pwd) in [
        (&mut host_reader, native_path.as_path(), None),
        (&mut rooted_reader, Path::new("/work/item"), Some(Path::new("/work"))),
        (
            &mut rooted_absolute_reader,
            Path::new("/work/item"),
            Some(Path::new("/work")),
        ),
        (&mut fixed_reader, native_path.as_path(), None),
    ] {
        let error = reader.read_to_end_limited(0).expect_err("nonempty input exceeds zero");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.operation(), LocalFileOperation::Read);
        assert_eq!(error.path(), Some(expected_path));
        assert_eq!(error.current_directory(), expected_pwd);
        let error = reader.verify_path_binding().expect_err("binding was not requested");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidState);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.path(), Some(expected_path));
        assert_eq!(error.current_directory(), expected_pwd);
    }
}

#[test]
fn test_observed_reader_host_relative_context_is_captured() {
    const CHILD: &str = "OBSERVED_READER_RELATIVE_CONTEXT_CHILD";
    const PASSED: &str = "observed-reader-relative-context-child-passed";

    if std::env::var_os(CHILD).is_some() {
        let opening_directory = std::env::current_dir().expect("opening directory");
        std::fs::write(opening_directory.join("item"), b"payload").expect("write item");
        std::fs::create_dir(opening_directory.join("other")).expect("create other");
        let filesystem = LocalFileSystem::host().expect("open Host");
        let mut reader = filesystem
            .open_observed_reader(Path::new("item"), &LocalObservedReadOptions::new())
            .expect("open relative reader");
        std::env::set_current_dir(opening_directory.join("other")).expect("change process directory");
        let error = reader.read_to_end_limited(0).expect_err("zero limit should fail");
        assert_eq!(error.path(), Some(opening_directory.join("item").as_path()));
        assert_eq!(error.current_directory(), Some(opening_directory.as_path()));
        println!("{PASSED}");
        return;
    }

    let fixture = tempfile::tempdir().expect("create fixture");
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("test_observed_reader_host_relative_context_is_captured")
        .arg("--nocapture")
        .env(CHILD, "1")
        .current_dir(fixture.path())
        .output()
        .expect("run child test process");
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("running 1 test"),
        "child did not run exactly one test: {stdout}"
    );
    assert!(
        stdout.contains(PASSED),
        "child did not reach assertion marker: {stdout}"
    );
}
