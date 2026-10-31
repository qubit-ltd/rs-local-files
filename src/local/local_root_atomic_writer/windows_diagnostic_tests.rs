// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::Path;

use super::LocalRootAtomicWriter;
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteOptions;
use crate::LocalAtomicWriteStage;
use crate::LocalDurabilityRequirement;
use crate::LocalRelativePath;

thread_local! {
    static STAGING_OPEN_ERROR: RefCell<Option<io::Error>> = const { RefCell::new(None) };
}

fn inject_staging_open_error(error: io::Error) {
    STAGING_OPEN_ERROR.with(|slot| *slot.borrow_mut() = Some(error));
}

pub(super) fn take_staging_open_error() -> Option<io::Error> {
    STAGING_OPEN_ERROR.with(|slot| slot.borrow_mut().take())
}

#[test]
fn test_windows_rooted_staging_create_failure_has_no_retained_path() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target.txt");
    fs::write(&target, b"original contents").expect("target fixture should be written");
    let root = crate::rooted::Root::open(directory.path()).expect("root authority should open");
    let root_handle = root.try_clone_authority().expect("root authority should clone");
    let requested_path = LocalRelativePath::new(Path::new("target.txt")).expect("relative target path should be valid");
    let options = LocalAtomicWriteOptions::new().with_durability(LocalDurabilityRequirement::NotRequired);

    inject_staging_open_error(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "injected staging open failure",
    ));
    let error = LocalRootAtomicWriter::new(&root_handle, root.path(), &requested_path, options)
        .expect_err("injected staging open failure should reject writer creation");

    assert_eq!(LocalAtomicWriteStage::CreateTemporaryFile, error.stage());
    assert_eq!(LocalAtomicDestinationState::Unchanged, error.destination_state());
    assert_eq!(requested_path.as_path(), error.path());
    assert_eq!(None, error.temporary_path());
    assert_eq!(
        io::ErrorKind::PermissionDenied,
        error.source_error().kind(),
        "native staging open error should remain the source"
    );
    assert!(
        !error.to_string().contains("staging path"),
        "a destination path must not be labeled as a staging path"
    );
    assert_eq!(
        b"original contents",
        fs::read(&target).expect("target should remain readable")
    );
}

#[cfg(feature = "test-support")]
#[test]
fn test_windows_rooted_commit_failure_retains_actual_staging_path() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target.txt");
    fs::write(&target, b"original contents").expect("target fixture should be written");
    let root = crate::rooted::Root::open(directory.path()).expect("root authority should open");
    let root_handle = root.try_clone_authority().expect("root authority should clone");
    let requested_path = LocalRelativePath::new(Path::new("target.txt")).expect("relative target path should be valid");
    let options = LocalAtomicWriteOptions::new().with_durability(LocalDurabilityRequirement::NotRequired);
    let writer = LocalRootAtomicWriter::new(&root_handle, root.path(), &requested_path, options)
        .expect("rooted staging file should be created");
    let staging_path = writer.staging_path().to_path_buf();
    let _fault = crate::local::install_test_fault("rooted-writer-identity-replace")
        .expect("commit-time destination replacement fault should install");

    let error = writer
        .commit_with_durability()
        .expect_err("destination replacement should fail commit");

    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(Some(staging_path.as_path()), error.temporary_path());
    assert_ne!(
        target, staging_path,
        "retained path must identify staging, not destination"
    );
    assert_eq!(
        b"competing destination",
        fs::read(&target).expect("competing destination should remain readable")
    );
}
