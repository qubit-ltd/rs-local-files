// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileErrorSource;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

fn followed_tree_options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_tree_source()
        .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .with_max_entries(32)
        .with_max_depth(8)
}

fn assert_containment_rejection(failure: qubit_local_files::outcome::LocalCopyFailure, source: &Path, component: &str) {
    assert_eq!(LocalFileErrorKind::InvalidPath, failure.error().kind());
    let io_error = match failure.error().typed_source() {
        Some(LocalFileErrorSource::Io(error)) => error,
        source => panic!("containment rejection should retain its I/O reason, got {source:?}"),
    };
    assert_eq!(std::io::ErrorKind::InvalidInput, io_error.kind());
    assert!(
        io_error
            .to_string()
            .contains("destination must not be inside the source")
    );
    assert!(failure.failed_source_path().is_some_and(|path| path.ends_with(source)));
    assert!(
        failure
            .failed_target_path()
            .is_some_and(|path| path.ends_with(component))
    );
}

#[test]
fn test_rooted_copy_rejects_link_to_destination_root() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("container/source");
    let destination = directory.path().join("container/target");
    fs::create_dir_all(&source).expect("source directory should be created");
    symlink("../target", source.join("target-link")).expect("destination link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let failure = rooted
        .copy_with_options(
            Path::new("container/source"),
            Path::new("container/target"),
            &followed_tree_options(),
        )
        .expect_err("link to destination root must be rejected");

    assert!(
        !destination.join("target-link").exists(),
        "dangerous destination layer must not be created"
    );
    assert_containment_rejection(failure, Path::new("container/target"), "target-link");
}

#[test]
fn test_rooted_copy_rejects_link_to_destination_ancestor() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("container/source");
    let destination = directory.path().join("container/target");
    fs::create_dir_all(&source).expect("source directory should be created");
    symlink("..", source.join("ancestor-link")).expect("ancestor link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let failure = rooted
        .copy_with_options(
            Path::new("container/source"),
            Path::new("container/target"),
            &followed_tree_options(),
        )
        .expect_err("link to destination ancestor must be rejected");

    assert!(
        !destination.join("ancestor-link").exists(),
        "dangerous destination layer must not be created"
    );
    assert_containment_rejection(failure, Path::new("container"), "ancestor-link");
}

#[test]
fn test_rooted_copy_preserves_ancestor_cycle_rejection() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("source");
    let destination = directory.path().join("target");
    fs::create_dir(&source).expect("source directory should be created");
    symlink(".", source.join("cycle")).expect("cycle link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let failure = rooted
        .copy_with_options(Path::new("source"), Path::new("target"), &followed_tree_options())
        .expect_err("ancestor cycle must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidPath, failure.error().kind());
    let io_error = match failure.error().typed_source() {
        Some(LocalFileErrorSource::Io(error)) => error,
        source => panic!("cycle rejection should retain its I/O reason, got {source:?}"),
    };
    assert!(io_error.to_string().contains("source directory cycle detected"));
    assert!(
        !destination.join("cycle").exists(),
        "cycle detection must precede destination preparation"
    );
}

#[test]
fn test_rooted_copy_follows_legal_sibling_directory_link() {
    let directory = tempdir().expect("temporary root should be created");
    let source = directory.path().join("container/source");
    fs::create_dir_all(&source).expect("source directory should be created");
    fs::create_dir_all(directory.path().join("container/sibling")).expect("sibling directory should be created");
    fs::write(directory.path().join("container/sibling/payload"), b"sibling data")
        .expect("sibling payload should be written");
    symlink("../sibling", source.join("sibling-link")).expect("sibling link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let outcome = rooted
        .copy_with_options(
            Path::new("container/source"),
            Path::new("container/target"),
            &followed_tree_options(),
        )
        .expect("link to a legal sibling directory should be followed");
    assert_eq!(1, outcome.stats().files());

    assert_eq!(
        b"sibling data",
        fs::read(directory.path().join("container/target/sibling-link/payload"))
            .expect("sibling payload should be copied")
            .as_slice(),
    );
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.

//    SPDX-License-Identifier: Apache-2.0

//    Licensed under the Apache License, Version 2.0.
// =============================================================================
