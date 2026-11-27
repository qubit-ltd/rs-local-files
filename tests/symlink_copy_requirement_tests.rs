// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalMetadataPreservePolicy;
use tempfile::tempdir;

fn options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions)
        .with_create_parent()
}

fn assert_unsupported_link_permissions(error: &qubit_local_files::outcome::LocalCopyFailure) {
    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.error().kind());
    assert_eq!(
        Some("portable permission metadata cannot be preserved for a symbolic-link source"),
        error.error().reason(),
    );
}

#[test]
fn test_host_copy_rejects_link_permissions_before_creating_parent() {
    let directory = tempdir().expect("temporary directory");
    let source = directory.path().join("source-link");
    let parent = directory.path().join("missing-parent");
    let target = parent.join("target-link");
    unix_fs::symlink("retained-target", &source).expect("source symlink");

    let error = LocalFileSystem::host()
        .expect("Host filesystem")
        .copy_with_options(&source, &target, &options())
        .expect_err("link permission preservation is unsupported");

    assert_unsupported_link_permissions(&error);
    assert!(
        fs::symlink_metadata(&source)
            .expect("source remains")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        Path::new("retained-target"),
        fs::read_link(source).expect("source link remains unchanged")
    );
    assert!(!parent.exists(), "destination parent must remain absent");
    assert!(!target.exists(), "destination link must remain absent");
}

#[test]
fn test_rooted_copy_rejects_link_permissions_before_creating_parent() {
    let directory = tempdir().expect("temporary directory");
    let source = directory.path().join("source-link");
    let parent = directory.path().join("missing-parent");
    let target = parent.join("target-link");
    unix_fs::symlink("retained-target", &source).expect("source symlink");

    let error = LocalFileSystem::rooted(directory.path())
        .expect("Rooted filesystem")
        .copy_with_options(
            Path::new("/source-link"),
            Path::new("/missing-parent/target-link"),
            &options(),
        )
        .expect_err("link permission preservation is unsupported");

    assert_unsupported_link_permissions(&error);
    assert!(
        fs::symlink_metadata(&source)
            .expect("source remains")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        Path::new("retained-target"),
        fs::read_link(source).expect("source link remains unchanged")
    );
    assert!(!parent.exists(), "destination parent must remain absent");
    assert!(!target.exists(), "destination link must remain absent");
}
