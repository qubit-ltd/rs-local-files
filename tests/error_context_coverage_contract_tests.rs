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

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
#[cfg(feature = "test-support")]
use qubit_local_files::options::LocalWriteMode;
#[cfg(feature = "test-support")]
use qubit_local_files::options::LocalWriteOptions;
use tempfile::tempdir;

#[test]
fn test_temp_affix_failures_keep_creation_context() {
    let directory = tempdir().expect("temporary parent should exist");
    let host = LocalFileSystem::host().expect("host should open");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root should open");
    for (filesystem, parent) in [(&host, directory.path()), (&rooted, Path::new("/"))] {
        for affix in ["bad/name", "bad\0name"] {
            let file_options = LocalTempFileOptions::new().with_parent(parent).with_prefix(affix);
            let error = filesystem
                .create_temp_file_with_options(&file_options)
                .expect_err("invalid file affix must fail before creation");
            assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
            assert_eq!(error.operation(), LocalFileOperation::CreateTempFile);
            assert_eq!(error.path(), Some(parent));

            let directory_options = LocalTempDirectoryOptions::new().with_parent(parent).with_suffix(affix);
            let error = filesystem
                .create_temp_directory_with_options(&directory_options)
                .expect_err("invalid directory affix must fail before creation");
            assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
            assert_eq!(error.operation(), LocalFileOperation::CreateTempDirectory);

            assert_eq!(error.path(), Some(parent));
        }
    }
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("parent should remain readable")
            .count(),
        0
    );
}

#[test]
fn test_rooted_listing_rejects_unrepresentable_deadline() {
    let directory = tempdir().expect("root should exist");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root should open");
    let error = filesystem
        .list_with_options(Path::new("/"), &LocalListOptions::new().with_deadline(Duration::MAX))
        .expect_err("unrepresentable deadline should fail immediately");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::List);
    assert_eq!(error.path(), Some(Path::new("/")));
}

#[test]
fn test_directory_qualified_canonicalize_checks_directory_kind() {
    let directory = tempdir().expect("fixture should exist");
    fs::write(directory.path().join("file"), b"payload").expect("regular file should exist");
    let host = LocalFileSystem::host().expect("host should open");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root should open");
    let canonical_parent = fs::canonicalize(directory.path()).expect("host parent should canonicalize");
    for (filesystem, parent, expected) in [
        (&host, directory.path(), canonical_parent.as_path()),
        (&rooted, Path::new("/"), Path::new("/")),
    ] {
        let qualified_directory = parent.join("./");
        assert_eq!(
            filesystem
                .canonicalize(&qualified_directory)
                .expect("directory should canonicalize"),
            expected
        );
        let error = filesystem
            .canonicalize(&parent.join("file/"))
            .expect_err("regular file cannot satisfy a directory-qualified operand");
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.kind(), LocalFileErrorKind::NotDirectory);
    }
}

#[cfg(feature = "test-support")]
#[test]
fn test_opened_handle_failures_preserve_native_cause_and_leave_file_intact() {
    use qubit_local_files::test_support::install_test_fault;

    let directory = tempdir().expect("fixture should exist");
    let path = directory.path().join("file");
    fs::write(&path, b"original payload").expect("regular file should exist");
    let filesystem = LocalFileSystem::host().expect("host should open");
    for point in ["file-handle-type", "file-handle-metadata"] {
        let fault = install_test_fault(point).expect("handle fault should install");
        let error = filesystem
            .open_reader(&path)
            .expect_err("opened handle validation should fail");
        assert_eq!(error.operation(), LocalFileOperation::OpenReader);
        assert_eq!(error.path(), Some(path.as_path()));
        assert!(error.typed_source().is_some());
        if point == "file-handle-type" {
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::InvalidInput);
        } else {
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
        }
        let error = filesystem
            .open_writer_with_options(&path, &LocalWriteOptions::new(LocalWriteMode::Append))
            .expect_err("append must validate the opened handle before returning it");
        assert_eq!(error.operation(), LocalFileOperation::OpenWriter);
        assert_eq!(error.path(), Some(path.as_path()));
        assert!(error.typed_source().is_some());
        assert_eq!(
            error.io_error_kind(),
            if point == "file-handle-type" {
                std::io::ErrorKind::InvalidInput
            } else {
                std::io::ErrorKind::Other
            }
        );
        drop(fault);
        assert_eq!(fs::read(&path).expect("file should remain intact"), b"original payload");
    }
}
