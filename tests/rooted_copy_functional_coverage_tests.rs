// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(all(feature = "test-support", unix))]

use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::options::LocalMetadataPreservePolicy;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::policy::LocalSymlinkPolicy;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

#[test]
fn test_rooted_tree_copy_replaces_entry_kinds_and_preserves_directory_permissions() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir_all(source.join("directory-replaces-file")).expect("source directory should be created");
    fs::write(source.join("directory-replaces-file/child"), b"nested").expect("nested source file should be written");
    fs::write(source.join("file-replaces-directory"), b"replacement").expect("source file should be written");
    symlink("file-replaces-directory", source.join("link")).expect("source link should be created");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o750))
        .expect("source directory permissions should be set");
    fs::set_permissions(
        source.join("directory-replaces-file"),
        fs::Permissions::from_mode(0o700),
    )
    .expect("nested source directory permissions should be set");

    fs::create_dir_all(target.join("file-replaces-directory/old-child"))
        .expect("conflicting target directory should be created");
    fs::write(target.join("directory-replaces-file"), b"old file").expect("conflicting target file should be written");
    fs::create_dir(target.join("link")).expect("conflicting link target directory should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let outcome = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
        )
        .expect("tree copy should replace conflicting entry kinds");

    assert_eq!(
        b"nested",
        fs::read(target.join("directory-replaces-file/child"))
            .unwrap()
            .as_slice()
    );
    assert_eq!(
        b"replacement",
        fs::read(target.join("file-replaces-directory")).unwrap().as_slice()
    );
    assert_eq!(
        Path::new("file-replaces-directory"),
        fs::read_link(target.join("link")).unwrap()
    );
    assert_eq!(0o750, fs::metadata(&target).unwrap().permissions().mode() & 0o7777);
    assert_eq!(
        0o700,
        fs::metadata(target.join("directory-replaces-file"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
    );
    assert_eq!(4, outcome.stats().overwritten());
}

#[test]
fn test_rooted_tree_copy_rejects_fifo_source_entry() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("source");
    fs::create_dir(&source).expect("source directory should be created");
    let fifo = source.join("pipe");
    let native_path = CString::new(fifo.as_os_str().as_bytes()).expect("fixture path should not contain NUL");
    assert_eq!(0, unsafe { libc::mkfifo(native_path.as_ptr(), 0o600) });
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect_err("tree copy should reject unsupported FIFO entries");

    assert_eq!(LocalFileErrorKind::Unsupported, failure.error().kind());
    assert!(directory.path().join("target").is_dir());
    assert!(!directory.path().join("target/pipe").exists());
}

#[test]
fn test_rooted_tree_copy_reports_directory_permission_preservation_failure() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir_all(directory.path().join("source/nested")).expect("nested source directory should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-set-permissions-native").expect("permission fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
        )
        .expect_err("permission preservation failure should fail tree copy");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert!(directory.path().join("target/nested").is_dir());
}

#[test]
fn test_rooted_tree_copy_rejects_followed_directory_cycle() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("source");
    fs::create_dir(&source).expect("source directory should be created");
    symlink(".", source.join("cycle")).expect("cycle link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        )
        .expect_err("a followed source directory cycle should fail the copy");

    assert_eq!(LocalFileErrorKind::InvalidPath, failure.error().kind());
    assert!(directory.path().join("target/cycle").is_dir());
}

#[test]
fn test_rooted_directory_copy_reports_failed_file_removal() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("source")).expect("source directory should be created");
    fs::write(directory.path().join("target"), b"keep").expect("conflicting destination file should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-remove-file-native").expect("remove fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("failed destination removal should fail directory preparation");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(b"keep", fs::read(directory.path().join("target")).unwrap().as_slice());
}

#[test]
fn test_rooted_directory_copy_reports_failed_replacement_creation() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("source")).expect("source directory should be created");
    fs::write(directory.path().join("target"), b"old").expect("conflicting destination file should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault =
        install_test_fault("rooted-copy-directory-create-native").expect("directory creation fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("failed replacement creation should fail directory preparation");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert!(!directory.path().join("target").exists());
}

#[test]
fn test_rooted_directory_copy_reports_replacement_statistics_overflow() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("source")).expect("source directory should be created");
    fs::write(directory.path().join("target"), b"old").expect("conflicting destination file should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault =
        install_test_fault("rooted-copy-statistics-overflow").expect("statistics overflow fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("statistics overflow should fail replacement preparation");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert!(directory.path().join("target").is_dir());
}

#[test]
fn test_rooted_symlink_copy_retains_file_destination_when_removal_fails() {
    let directory = tempdir().expect("temporary root should be created");
    symlink("referent", directory.path().join("source")).expect("source link should be created");
    fs::write(directory.path().join("target"), b"keep").expect("conflicting destination file should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-remove-file-native").expect("remove fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_conflict(LocalCopyConflictPolicy::Overwrite),
        )
        .expect_err("failed destination removal should fail symlink replacement");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(b"keep", fs::read(directory.path().join("target")).unwrap().as_slice());
}

#[test]
fn test_rooted_symlink_copy_retains_directory_destination_when_removal_fails() {
    let directory = tempdir().expect("temporary root should be created");
    symlink("referent", directory.path().join("source")).expect("source link should be created");
    fs::create_dir(directory.path().join("target")).expect("conflicting target directory should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-remove-tree-native").expect("remove fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("failed directory removal should fail symlink replacement");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert!(directory.path().join("target").is_dir());
}

#[test]
fn test_rooted_symlink_copy_reports_publication_failure() {
    let directory = tempdir().expect("temporary root should be created");
    symlink("referent", directory.path().join("source")).expect("source link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-symlink-create").expect("publication fault should install");

    let failure = rooted
        .copy_with_options(Path::new("source"), Path::new("target"), &LocalCopyOptions::new())
        .expect_err("injected symlink publication should fail");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert!(!directory.path().join("target").exists());
}

#[test]
fn test_rooted_file_copy_reports_missing_destination_parent() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload").expect("source file should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("missing/target"),
            &LocalCopyOptions::new(),
        )
        .expect_err("missing destination parent should fail staged writer creation");

    assert_eq!(LocalFileErrorKind::NotFound, failure.error().kind());
    assert!(directory.path().join("source").exists());
}

#[test]
fn test_rooted_file_copy_reports_content_failure_after_replacement() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload").expect("source file should be written");
    fs::write(directory.path().join("target"), b"old").expect("old target should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-file-contents-native").expect("content copy fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_conflict(LocalCopyConflictPolicy::Overwrite),
        )
        .expect_err("injected content failure should fail the copy");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(b"old", fs::read(directory.path().join("target")).unwrap().as_slice());
}

#[test]
fn test_rooted_file_copy_reports_expired_deadline() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload").expect("source file should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_deadline(Duration::ZERO),
        )
        .expect_err("expired deadline should stop copy before reading content");

    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert!(!directory.path().join("target").exists());
}
