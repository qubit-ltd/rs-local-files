// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod directory;

use std::fs;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;

#[test]
fn test_directory_handle_list_zero_deadline_times_out_without_changes() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir(fixture.path().join("child")).expect("child should exist");
    fs::write(fixture.path().join("child/payload"), b"unchanged").expect("payload should exist");
    let handle = LocalFileSystem::rooted(fixture.path())
        .expect("rooted filesystem should open")
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("directory handle should open");

    let error = handle
        .list_with_options(
            Path::new("child"),
            &LocalListOptions::new().with_deadline(Duration::ZERO),
        )
        .expect_err("zero deadline should time out");

    assert_eq!(error.kind(), LocalFileErrorKind::Io);
    assert_eq!(error.operation(), LocalFileOperation::List);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(
        fs::read(fixture.path().join("child/payload")).expect("payload should remain"),
        b"unchanged"
    );
}
