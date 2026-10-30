// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(all(feature = "test-support", unix))]

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

fn copy_file_over_replaced_tree(directory: &Path, fault: &str) -> qubit_local_files::outcome::LocalCopyFailure {
    let directory = fs::canonicalize(directory).expect("temporary root should canonicalize");
    fs::write(directory.join("source"), b"replacement").expect("source file should be written");
    fs::create_dir_all(directory.join("target/first")).expect("target subtree should be created");
    fs::write(directory.join("target/first/entry"), b"first").expect("first target should be written");
    fs::write(directory.join("target/second"), b"second").expect("second target should be written");
    let rooted = LocalFileSystem::rooted(&directory).expect("root authority should open");
    let _fault = install_test_fault(fault).expect("copy fault should install");

    rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("injected copy fault should fail")
}

#[test]
fn test_partial_tree_removal_is_reported_with_zero_copy_statistics() {
    let directory = tempdir().expect("temporary root should be created");
    let failure = copy_file_over_replaced_tree(directory.path(), "temp-directory-remove-second");

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().directories());
    let surviving_leaves = usize::from(directory.path().join("target/first/entry").exists())
        + usize::from(directory.path().join("target/second").exists());
    assert_eq!(1, surviving_leaves);
}

#[test]
fn test_failure_before_tree_removal_is_unchanged() {
    let directory = tempdir().expect("temporary root should be created");
    let failure = copy_file_over_replaced_tree(directory.path(), "rooted-copy-remove-tree-native");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(2, fs::read_dir(directory.path().join("target")).unwrap().count());
}

#[test]
fn test_create_parent_remains_after_source_open_failure_without_destination_publication() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"replacement").expect("source file should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-source-open").expect("source-open fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("new-parent/target"),
            &LocalCopyOptions::new().with_create_parent(),
        )
        .expect_err("injected source-open fault should fail");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert!(directory.path().join("new-parent").is_dir());
    assert!(!directory.path().join("new-parent/target").exists());
    assert_eq!(LocalFileErrorKind::PermissionDenied, failure.error().kind());
}

#[test]
fn test_writer_open_failure_preserves_conflicting_tree() {
    let directory = tempdir().expect("temporary root should be created");
    let failure = copy_file_over_replaced_tree(directory.path(), "rooted-copy-writer-open");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(
        b"first",
        fs::read(directory.path().join("target/first/entry"))
            .unwrap()
            .as_slice()
    );
    assert_eq!(
        b"second",
        fs::read(directory.path().join("target/second")).unwrap().as_slice()
    );
    assert_eq!(LocalFileErrorKind::PermissionDenied, failure.error().kind());
}

#[test]
fn test_copy_contents_failure_preserves_conflicting_tree() {
    let directory = tempdir().expect("temporary root should be created");
    let _failure = copy_file_over_replaced_tree(directory.path(), "rooted-copy-file-contents-native");

    assert_eq!(
        b"first",
        fs::read(directory.path().join("target/first/entry"))
            .unwrap()
            .as_slice()
    );
    assert_eq!(
        b"second",
        fs::read(directory.path().join("target/second")).unwrap().as_slice()
    );
}

#[test]
fn test_copy_failure_retains_staging_cleanup_error_context() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"replacement").expect("source file should be written");
    fs::create_dir_all(directory.path().join("target/first")).expect("target subtree should be created");
    fs::write(directory.path().join("target/first/entry"), b"first").expect("first target should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault =
        install_test_fault("rooted-copy-file-contents-cleanup").expect("copy and cleanup fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("injected copy and cleanup failures should be retained");

    assert!(failure.staging_path().is_some());
    assert_eq!(
        Some(LocalFileErrorKind::Io),
        failure
            .cleanup_error()
            .map(qubit_local_files::error::LocalFileError::kind)
    );
    assert_eq!(
        b"first",
        fs::read(directory.path().join("target/first/entry"))
            .unwrap()
            .as_slice()
    );
}

#[test]
fn test_destination_replaced_after_staging_is_preserved() {
    let directory = tempdir().expect("temporary root should be created");
    let failure = copy_file_over_replaced_tree(directory.path(), "rooted-copy-replacement-before-remove");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(
        b"replacement sentinel",
        fs::read(directory.path().join("target")).unwrap().as_slice()
    );
    assert_eq!(
        b"first",
        fs::read(directory.path().join("rooted-copy-displaced-target/first/entry"))
            .unwrap()
            .as_slice()
    );
    assert_eq!(
        b"second",
        fs::read(directory.path().join("rooted-copy-displaced-target/second"))
            .unwrap()
            .as_slice()
    );
}

#[test]
fn test_competitor_created_after_tree_removal_is_not_overwritten() {
    let directory = tempdir().expect("temporary root should be created");
    let failure = copy_file_over_replaced_tree(directory.path(), "rooted-copy-race-before-writer-commit");

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert_eq!(
        b"concurrent sentinel",
        fs::read(directory.path().join("target")).unwrap().as_slice()
    );
}

#[test]
fn test_directory_create_failure_after_file_removal_is_partial() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("source")).expect("source directory should be created");
    fs::write(directory.path().join("target"), b"old").expect("old target should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-replacement-directory-create").expect("copy fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("directory creation fault should fail");

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(!directory.path().join("target").exists());
    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().directories());
}
