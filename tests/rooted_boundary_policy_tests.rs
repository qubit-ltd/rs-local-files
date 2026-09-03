// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Rooted boundary-policy integration tests.

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use tempfile::tempdir;

/// Verifies rooted temporary resources reject invalid retry and naming
/// policies before allocating an owned sandbox.
#[test]
fn rooted_temp_resources_validate_attempt_and_affix_policies() {
    let directory = tempdir().expect("temporary root should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let file_attempt = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_max_attempts(0))
        .expect_err("a zero file retry budget must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidOptions, file_attempt.kind());

    let directory_attempt = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_max_attempts(0))
        .expect_err("a zero directory retry budget must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidOptions, directory_attempt.kind());

    let file_affix = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_prefix("nested/"))
        .expect_err("a file prefix containing a separator must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidOptions, file_affix.kind());

    let directory_affix = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_suffix("/nested"))
        .expect_err("a directory suffix containing a separator must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidOptions, directory_affix.kind());
}

/// Verifies rooted temporary parents must already be directories unless the
/// caller explicitly requests recursive parent creation.
#[test]
fn rooted_temp_resources_validate_parent_state() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("file-parent"), b"payload").expect("file parent fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let missing_file_parent = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(Path::new("missing")))
        .expect_err("a missing file parent must be rejected");
    assert_eq!(LocalFileErrorKind::NotFound, missing_file_parent.kind());

    let missing_directory_parent = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(Path::new("missing")))
        .expect_err("a missing directory parent must be rejected");
    assert_eq!(LocalFileErrorKind::NotFound, missing_directory_parent.kind());

    let file_parent = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(Path::new("file-parent")))
        .expect_err("a regular file cannot be a temporary-file parent");
    assert_eq!(LocalFileErrorKind::NotDirectory, file_parent.kind());

    let directory_parent = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(Path::new("file-parent")))
        .expect_err("a regular file cannot be a temporary-directory parent");
    assert_eq!(LocalFileErrorKind::NotDirectory, directory_parent.kind());
}

/// Verifies rooted directory creation distinguishes existing directories,
/// incompatible entries, and missing ancestors.
#[test]
fn rooted_directory_creation_enforces_existing_entry_policies() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("existing")).expect("existing directory should be created");
    fs::write(directory.path().join("file"), b"payload").expect("file fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let existing = filesystem
        .create_directory_with_options(Path::new("existing"), &LocalCreateDirectoryOptions::new())
        .expect_err("an existing directory must fail without exists-ok");
    assert_eq!(LocalFileErrorKind::AlreadyExists, existing.kind());

    let accepted = filesystem
        .create_directory_with_options(
            Path::new("existing"),
            &LocalCreateDirectoryOptions::new().with_exists_ok(),
        )
        .expect("exists-ok should accept an existing directory");
    assert!(!accepted.created());

    let conflict = filesystem
        .create_directory_with_options(Path::new("file"), &LocalCreateDirectoryOptions::new().with_exists_ok())
        .expect_err("exists-ok must not accept a regular file");
    assert_eq!(LocalFileErrorKind::TypeConflict, conflict.kind());

    let missing_parent = filesystem
        .create_directory_with_options(Path::new("missing/child"), &LocalCreateDirectoryOptions::new())
        .expect_err("non-recursive creation must not synthesize ancestors");
    assert_eq!(LocalFileErrorKind::NotFound, missing_parent.kind());

    let recursive = filesystem
        .create_directory_with_options(
            Path::new("tree/branch/leaf"),
            &LocalCreateDirectoryOptions::new().with_recursive().with_exists_ok(),
        )
        .expect("recursive creation should create every missing component");
    assert!(recursive.created());
    assert!(directory.path().join("tree/branch/leaf").is_dir());
}

/// Verifies rooted listing validates both the virtual root and a requested
/// final entry before producing a walker.
#[test]
fn rooted_listing_rejects_non_directory_start_entries() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("file"), b"payload").expect("file fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let root_entries = filesystem
        .list(Path::new(""))
        .expect("the empty path should resolve to the rooted virtual root")
        .collect::<Result<Vec<_>, _>>()
        .expect("root entries should be readable");
    assert_eq!(1, root_entries.len());

    let error = filesystem
        .list(Path::new("file"))
        .expect_err("a regular file cannot be a listing start");
    assert_eq!(LocalFileErrorKind::TypeConflict, error.kind());
}
