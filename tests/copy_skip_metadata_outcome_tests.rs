// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(unix)]

//! A skipped copy must not report metadata preservation as applied.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::options::LocalMetadataPreservePolicy;
use tempfile::tempdir;

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("permissions should be set");
}

fn skip_options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_type_conflict(LocalCopyTypeConflictPolicy::Skip)
        .with_conflict(LocalCopyConflictPolicy::Skip)
        .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions)
}

#[test]
fn test_host_type_conflict_skip_does_not_report_metadata_preservation() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::write(&source, b"source").expect("source should exist");
    fs::create_dir(&target).expect("target directory should exist");
    set_mode(&source, 0o600);
    set_mode(&target, 0o700);

    let outcome = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(&source, &target, &skip_options())
        .expect("type conflict should be skipped");

    assert_eq!(1, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().files());
    assert_eq!(0, outcome.stats().bytes());
    assert_eq!(LocalMetadataPreservePolicy::None, outcome.metadata_preservation());
    assert_eq!(
        0o700,
        fs::metadata(&target)
            .expect("target should remain")
            .permissions()
            .mode()
            & 0o777
    );
    assert_eq!(b"source", fs::read(&source).expect("source should remain").as_slice());
}

#[test]
fn test_host_file_conflict_skip_does_not_report_metadata_preservation() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::write(&source, b"source").expect("source should exist");
    fs::write(&target, b"existing").expect("target should exist");
    set_mode(&source, 0o600);
    set_mode(&target, 0o700);

    let outcome = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Skip)
                .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
        )
        .expect("file conflict should be skipped");

    assert_eq!(1, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().files());
    assert_eq!(0, outcome.stats().bytes());
    assert_eq!(LocalMetadataPreservePolicy::None, outcome.metadata_preservation());
    assert_eq!(
        0o700,
        fs::metadata(&target)
            .expect("target should remain")
            .permissions()
            .mode()
            & 0o777
    );
    assert_eq!(
        b"existing",
        fs::read(&target).expect("target contents should remain").as_slice()
    );
}

#[test]
fn test_rooted_type_conflict_skip_does_not_report_metadata_preservation() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::write(&source, b"source").expect("source should exist");
    fs::create_dir(&target).expect("target directory should exist");
    set_mode(&source, 0o600);
    set_mode(&target, 0o700);

    let outcome = LocalFileSystem::rooted(fixture.path())
        .expect("rooted filesystem should open")
        .copy_with_options(Path::new("source"), Path::new("target"), &skip_options())
        .expect("type conflict should be skipped");

    assert_eq!(1, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().files());
    assert_eq!(0, outcome.stats().bytes());
    assert_eq!(LocalMetadataPreservePolicy::None, outcome.metadata_preservation());
    assert_eq!(
        0o700,
        fs::metadata(&target)
            .expect("target should remain")
            .permissions()
            .mode()
            & 0o777
    );
    assert_eq!(b"source", fs::read(&source).expect("source should remain").as_slice());
}

#[test]
fn test_host_symlink_skip_does_not_report_metadata_preservation() {
    use std::os::unix::fs::symlink;

    let fixture = tempdir().expect("fixture should exist");
    let link_target = fixture.path().join("link-target");
    let source = fixture.path().join("source-link");
    let target = fixture.path().join("target");
    fs::write(&link_target, b"referent").expect("link target should exist");
    symlink(&link_target, &source).expect("source symlink should exist");
    fs::write(&target, b"existing").expect("existing destination should exist");
    set_mode(&link_target, 0o600);
    set_mode(&target, 0o700);

    let outcome = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(&source, &target, &skip_options())
        .expect("symlink conflict should be skipped");

    assert_eq!(1, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().files());
    assert_eq!(0, outcome.stats().bytes());
    assert_eq!(LocalMetadataPreservePolicy::None, outcome.metadata_preservation());
    assert_eq!(
        0o700,
        fs::metadata(&target)
            .expect("target should remain")
            .permissions()
            .mode()
            & 0o777
    );
    assert_eq!(
        b"existing",
        fs::read(&target).expect("target contents should remain").as_slice()
    );
}

#[test]
fn test_successful_host_copy_reports_applied_permissions() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::write(&source, b"copied").expect("source should exist");
    set_mode(&source, 0o600);

    let outcome = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new().with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
        )
        .expect("copy should succeed");

    assert_eq!(1, outcome.stats().files());
    assert_eq!(6, outcome.stats().bytes());
    assert_eq!(0, outcome.stats().skipped());
    assert_eq!(
        LocalMetadataPreservePolicy::Permissions,
        outcome.metadata_preservation()
    );
    assert_eq!(
        0o600,
        fs::metadata(&target).expect("target should exist").permissions().mode() & 0o777
    );
}

#[test]
fn test_host_empty_directory_copy_reports_applied_permissions() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).expect("source directory should exist");
    set_mode(&source, 0o500);

    let outcome = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new().with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
        )
        .expect("empty directory copy should succeed");

    assert_eq!(0, outcome.stats().files());
    assert!(outcome.stats().directories() > 0);
    assert_eq!(0, outcome.stats().skipped());
    assert_eq!(
        LocalMetadataPreservePolicy::Permissions,
        outcome.metadata_preservation()
    );
    assert_eq!(
        0o500,
        fs::metadata(&target)
            .expect("target directory should exist")
            .permissions()
            .mode()
            & 0o777
    );
}

#[test]
fn test_rooted_empty_directory_copy_reports_applied_permissions() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).expect("source directory should exist");
    set_mode(&source, 0o500);

    let outcome = LocalFileSystem::rooted(fixture.path())
        .expect("rooted filesystem should open")
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
        )
        .expect("empty directory copy should succeed");

    assert_eq!(0, outcome.stats().files());
    assert!(outcome.stats().directories() > 0);
    assert_eq!(0, outcome.stats().skipped());
    assert_eq!(
        LocalMetadataPreservePolicy::Permissions,
        outcome.metadata_preservation()
    );
    assert_eq!(
        0o500,
        fs::metadata(&target)
            .expect("target directory should exist")
            .permissions()
            .mode()
            & 0o777
    );
}
