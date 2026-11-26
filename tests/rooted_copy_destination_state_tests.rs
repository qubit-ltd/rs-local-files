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
use qubit_local_files::policy::LocalAtomicityRequirement;
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
    assert_eq!(
        2,
        fs::read_dir(directory.path().join("target"))
            .expect("target directory should remain readable")
            .count()
    );
}

#[test]
fn test_required_atomicity_rechecks_raced_directory_destination_before_removal() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"source").unwrap();
    fs::write(directory.path().join("target"), b"old target").unwrap();
    let rooted = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let _fault = install_test_fault("rooted-copy-target-becomes-directory").expect("race injection installs");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_atomicity(LocalAtomicityRequirement::Required),
        )
        .expect_err("the backend must reject a raced non-atomic replacement");

    assert_eq!(LocalFileErrorKind::RequirementNotMet, failure.error().kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert!(directory.path().join("target").is_dir());
    assert_eq!(0, fs::read_dir(directory.path().join("target")).unwrap().count());
}

#[test]
fn test_required_atomicity_rechecks_actual_rooted_source_kind() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"source").unwrap();
    let rooted = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let _fault = install_test_fault("rooted-copy-source-becomes-symlink").expect("race injection installs");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_entry_source()
                .with_atomicity(LocalAtomicityRequirement::Required),
        )
        .expect_err("the selected symlink source cannot satisfy required atomicity");

    assert_eq!(LocalFileErrorKind::RequirementNotMet, failure.error().kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert!(
        fs::symlink_metadata(directory.path().join("source"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!directory.path().join("target").exists());
}

#[test]
fn test_rooted_copy_reports_method_for_backend_selected_source_kind() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"preflight file").unwrap();
    let rooted = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let _fault = install_test_fault("rooted-copy-source-becomes-directory").expect("race injection installs");

    let outcome = rooted
        .copy_with_options(Path::new("source"), Path::new("target"), &LocalCopyOptions::new())
        .expect("Auto mode should copy the actual directory source");

    assert_eq!(qubit_local_files::outcome::LocalCopyMethod::Recursive, outcome.method());
    assert_eq!(1, outcome.stats().directories());
    assert!(directory.path().join("target").is_dir());
}

#[test]
fn test_rooted_copy_race_rewrite_rejects_directory_entries_without_mutation() {
    for (fault, directory_entry) in [
        ("rooted-copy-target-becomes-directory", false),
        ("rooted-copy-source-becomes-symlink", true),
        ("rooted-copy-source-becomes-directory", true),
    ] {
        let directory = tempdir().expect("temporary root should be created");
        if directory_entry {
            fs::create_dir(directory.path().join("source")).expect("source directory should be created");
        } else {
            fs::write(directory.path().join("source"), b"source").expect("source should be written");
            fs::create_dir(directory.path().join("target")).expect("target directory should be created");
        }
        let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
        let _fault = install_test_fault(fault).expect("race injection installs");
        let options = if directory_entry {
            LocalCopyOptions::new()
        } else {
            LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        };

        let failure = rooted
            .copy_with_options(Path::new("source"), Path::new("target"), &options)
            .expect_err("race rewrite must fail when its entry is a directory");

        assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
        if directory_entry {
            assert!(directory.path().join("source").is_dir());
            assert!(!directory.path().join("target").exists());
        } else {
            assert_eq!(b"source", fs::read(directory.path().join("source")).unwrap().as_slice());
            assert!(directory.path().join("target").is_dir());
        }
    }
}

#[test]
fn test_create_parent_effect_is_reported_after_source_open_failure_without_destination_publication() {
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

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(directory.path().join("new-parent").is_dir());
    assert!(!directory.path().join("new-parent/target").exists());
    assert_eq!(LocalFileErrorKind::PermissionDenied, failure.error().kind());
}

#[test]
fn test_host_regular_file_copy_failure_reports_created_parent_effect() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload").expect("source should be written");
    let _fault = install_test_fault("copy-staging-copy").expect("staging copy fault should install");

    let failure = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(
            &directory.path().join("source"),
            &directory.path().join("new-parent/target"),
            &LocalCopyOptions::new().with_create_parent(),
        )
        .expect_err("injected staging copy fault should fail");

    assert!(directory.path().join("new-parent").is_dir());
    assert!(!directory.path().join("new-parent/target").exists());
    assert!(failure.staging_path().is_none());
    assert!(failure.cleanup_error().is_none());
    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
}

#[test]
fn test_host_tree_copy_failure_reports_created_parent_effect() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("source-tree")).expect("source tree should be created");
    fs::write(directory.path().join("source-tree/entry"), b"payload").expect("entry should be written");
    let _fault = install_test_fault("copy-staging-copy").expect("staging copy fault should install");

    let failure = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(
            &directory.path().join("source-tree"),
            &directory.path().join("new-parent/target"),
            &LocalCopyOptions::new().with_tree_source().with_create_parent(),
        )
        .expect_err("injected tree child copy fault should fail");

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(directory.path().join("new-parent").is_dir());
    assert!(directory.path().join("new-parent/target").is_dir());
}

#[test]
fn test_rooted_tree_copy_failure_reports_created_parent_effect() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("source-tree")).expect("source tree should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-directory-create").expect("directory fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source-tree"),
            Path::new("new-parent/target"),
            &LocalCopyOptions::new().with_tree_source().with_create_parent(),
        )
        .expect_err("injected tree destination create fault should fail");

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(directory.path().join("new-parent").is_dir());
    assert!(!directory.path().join("new-parent/target").exists());
}

#[test]
fn test_rooted_link_copy_failure_reports_created_parent_effect() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    symlink("missing", directory.path().join("source")).expect("source link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-symlink-create").expect("link fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("new-parent/target"),
            &LocalCopyOptions::new().with_create_parent(),
        )
        .expect_err("injected link publication fault should fail");

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(directory.path().join("new-parent").is_dir());
    assert!(!directory.path().join("new-parent/target").exists());
}

#[cfg(unix)]
#[test]
fn test_host_link_copy_failure_reports_created_parent_without_publication() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    symlink("referent", directory.path().join("source")).expect("source link should be created");
    let _fault = install_test_fault("local-fs-copy-symlink-create").expect("link fault should install");

    let failure = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(
            &directory.path().join("source"),
            &directory.path().join("new-parent/target"),
            &LocalCopyOptions::new().with_create_parent(),
        )
        .expect_err("injected link creation fault should fail");

    assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
    assert!(directory.path().join("new-parent").is_dir());
    assert!(!directory.path().join("new-parent/target").exists());
}

#[test]
fn test_writer_open_failure_preserves_conflicting_tree() {
    let directory = tempdir().expect("temporary root should be created");
    let failure = copy_file_over_replaced_tree(directory.path(), "rooted-copy-writer-open");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(
        b"first",
        fs::read(directory.path().join("target/first/entry"))
            .expect("first target should remain readable")
            .as_slice()
    );
    assert_eq!(
        b"second",
        fs::read(directory.path().join("target/second"))
            .expect("second target should remain readable")
            .as_slice()
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
            .expect("first target should remain readable")
            .as_slice()
    );
    assert_eq!(
        b"second",
        fs::read(directory.path().join("target/second"))
            .expect("second target should remain readable")
            .as_slice()
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
            .expect("first target should remain readable")
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
        fs::read(directory.path().join("target"))
            .expect("replacement target should remain readable")
            .as_slice()
    );
    assert_eq!(
        b"first",
        fs::read(directory.path().join("rooted-copy-displaced-target/first/entry"))
            .expect("displaced first target should remain readable")
            .as_slice()
    );
    assert_eq!(
        b"second",
        fs::read(directory.path().join("rooted-copy-displaced-target/second"))
            .expect("displaced second target should remain readable")
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
        fs::read(directory.path().join("target"))
            .expect("concurrent target should remain readable")
            .as_slice()
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
