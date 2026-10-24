// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(all(feature = "test-support", unix))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalMetadataPreservePolicy;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

fn rooted_copy_options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions)
        .with_durability(LocalDurabilityRequirement::Required)
}

#[test]
fn test_rooted_copy_publishes_source_permissions_with_content() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"new content").expect("source should be written");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).expect("source mode should be set");
    fs::write(&target, b"old content").expect("old destination should be written");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("destination mode should be set");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let _outcome = rooted
        .copy_with_options(Path::new("source"), Path::new("target"), &rooted_copy_options())
        .expect("copy should succeed");

    assert_eq!(
        b"new content",
        fs::read(&target).expect("new target should be readable").as_slice()
    );
    assert_eq!(
        0o600,
        fs::metadata(&target)
            .expect("new target metadata should be readable")
            .permissions()
            .mode()
            & 0o7777
    );
}

#[test]
fn test_rooted_copy_permission_failure_keeps_existing_destination() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"new content").expect("source should be written");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).expect("source mode should be set");
    fs::write(&target, b"old content").expect("old destination should be written");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("destination mode should be set");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let _fault = install_test_fault("rooted-copy-set-permissions").expect("permission fault should install");

    let failure = rooted
        .copy_with_options(Path::new("source"), Path::new("target"), &rooted_copy_options())
        .expect_err("permission application fault should fail the copy");

    assert_eq!(LocalFileErrorKind::PermissionDenied, failure.error().kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(
        b"old content",
        fs::read(&target).expect("old target should remain").as_slice()
    );
    assert_eq!(
        0o644,
        fs::metadata(&target)
            .expect("old target metadata should remain")
            .permissions()
            .mode()
            & 0o7777
    );
}
