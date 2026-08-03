// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    path::Path,
};

use qubit_local_files::{
    LocalFileErrorKind,
    LocalFileSystem,
    LocalTempFileOptions,
};
use tempfile::tempdir;

/// Verifies rooted temporary parents accept an existing descendant directory.
#[test]
fn test_rooted_path_support_accepts_existing_temp_parent() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("parent"))
        .expect("temporary parent should be created");
    let rooted = LocalFileSystem::rooted(directory.path())
        .expect("root authority should open");

    let temporary = rooted
        .create_temp_file(
            &LocalTempFileOptions::new().with_parent(Path::new("parent")),
        )
        .expect("existing rooted parent should be accepted");
    assert!(temporary.path().starts_with(Path::new("parent")));
}

/// Verifies rooted temporary-parent validation distinguishes missing and
/// non-directory descendants.
#[test]
fn test_rooted_path_support_rejects_invalid_temp_parents() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("file");
    fs::write(&file, b"not a directory")
        .expect("file fixture should be written");
    let rooted = LocalFileSystem::rooted(directory.path())
        .expect("root authority should open");

    let missing = rooted
        .create_temp_file(
            &LocalTempFileOptions::new().with_parent(Path::new("missing")),
        )
        .expect_err("missing rooted parent should be rejected");
    assert_eq!(LocalFileErrorKind::NotFound, missing.kind());

    let not_directory = rooted
        .create_temp_file(
            &LocalTempFileOptions::new().with_parent(Path::new("file")),
        )
        .expect_err("file rooted parent should be rejected");
    assert_eq!(LocalFileErrorKind::NotDirectory, not_directory.kind());
}

/// Verifies rooted temporary-name validation maps invalid affixes to options.
#[test]
fn test_rooted_path_support_rejects_invalid_temp_affixes() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = LocalFileSystem::rooted(directory.path())
        .expect("root authority should open");

    let error = rooted
        .create_temp_file(&LocalTempFileOptions::new().with_prefix("bad/name"))
        .expect_err("path separators must be rejected in temporary prefixes");
    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
}

/// Verifies rooted operations preserve invalid descendant path context.
#[test]
fn test_rooted_path_support_rejects_escape_paths() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = LocalFileSystem::rooted(directory.path())
        .expect("root authority should open");

    let error = rooted
        .metadata(Path::new("../escape"))
        .expect_err("parent traversal must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(Some(Path::new("../escape")), error.path());
}
