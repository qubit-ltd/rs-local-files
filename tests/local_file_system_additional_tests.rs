// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::outcome::LocalFileKind;
use tempfile::tempdir;

/// Verifies metadata errors retain the caller-visible path when an
/// intermediate symbolic link is resolved for native inspection.
#[cfg(unix)]
#[test]
fn test_local_file_system_metadata_preserves_logical_error_path() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    fs::create_dir(&target).expect("target directory should be created");
    symlink(&target, &link).expect("intermediate symlink should be created");
    let missing = link.join("missing");

    let error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&missing)
        .expect_err("missing metadata should return an error");

    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
    assert_eq!(Some(missing.as_path()), error.path());
}

/// Verifies host metadata reports regular files and preserves a missing-path
/// error rather than synthesizing metadata.
#[test]
fn test_local_file_system_metadata_reports_file_and_missing_path_error() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("payload");
    fs::write(&file, b"payload").expect("payload fixture should be written");

    let metadata = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&file)
        .expect("file metadata should be available");
    assert_eq!(LocalFileKind::File, metadata.kind());
    assert_eq!(7, metadata.len());

    let missing = directory.path().join("missing");
    let error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&missing)
        .expect_err("missing metadata should return an error");
    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
    assert_eq!(Some(missing.as_path()), error.path());
}

/// Verifies directory creation distinguishes absent parents, duplicate entries,
/// and an existing non-directory target.
#[test]
fn test_local_file_system_create_directory_reports_policy_and_type_errors() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing_parent_target = directory.path().join("missing/child");
    let missing_parent_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&missing_parent_target, &LocalCreateDirectoryOptions::new())
        .expect_err("non-recursive creation must reject a missing parent");
    assert!(matches!(
        missing_parent_error.kind(),
        LocalFileErrorKind::NotFound | LocalFileErrorKind::Io
    ));

    let existing = directory.path().join("existing");
    fs::create_dir(&existing).expect("existing directory should be created");
    let duplicate_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&existing, &LocalCreateDirectoryOptions::new())
        .expect_err("existing directories require explicit acceptance");
    assert_eq!(LocalFileErrorKind::AlreadyExists, duplicate_error.kind());

    let file = directory.path().join("file");
    fs::write(&file, b"payload").expect("file fixture should be written");
    let type_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&file, &LocalCreateDirectoryOptions::new().with_exists_ok())
        .expect_err("a regular file cannot satisfy a directory request");
    assert_eq!(LocalFileErrorKind::TypeConflict, type_error.kind());
}

/// Verifies host delete operations honor missing-entry policy and reject an
/// entry whose kind conflicts with the requested deletion operation.
#[test]
fn test_local_file_system_delete_handles_missing_and_type_conflicts() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing_file = directory.path().join("missing-file");
    let missing_outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_file_with_options(&missing_file, &LocalDeleteOptions::new().with_missing_ok())
        .expect("missing file should be accepted by policy");
    assert!(!missing_outcome.deleted());

    let child_directory = directory.path().join("directory");
    fs::create_dir(&child_directory).expect("directory fixture should be created");
    let file_delete_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_file_with_options(&child_directory, &LocalDeleteOptions::new())
        .expect_err("directory must not be deleted as a file");
    assert_eq!(LocalFileErrorKind::TypeConflict, file_delete_error.kind());

    let file = directory.path().join("file");
    fs::write(&file, b"payload").expect("file fixture should be written");
    let directory_delete_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_directory_with_options(&file, &LocalDeleteOptions::new())
        .expect_err("regular files must not be deleted as directories");
    assert_eq!(LocalFileErrorKind::TypeConflict, directory_delete_error.kind());
}

/// Verifies host reader reports both missing entries and final entry kinds
/// before attempting a native file read.
#[test]
fn test_local_file_system_open_reader_reports_missing_and_directory_errors() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&directory.path().join("missing"), &LocalReadOptions::new())
        .expect_err("missing files must not open as readers");
    assert_eq!(LocalFileErrorKind::NotFound, missing_error.kind());

    let directory_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(directory.path(), &LocalReadOptions::new())
        .expect_err("directories must not open as readers");
    assert_eq!(LocalFileErrorKind::TypeConflict, directory_error.kind());

    assert!(directory.path().is_dir());
}
