// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::{
    copy,
    rooted,
};

/// Verifies that rooted copy stages and installs one regular file.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_installs_complete_contents() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"complete payload")
        .expect("the source should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let statistics = root
        .copy(&source, &destination, copy::Options::new())
        .expect("the rooted file should be copied");

    assert_eq!(1, statistics.files());
    assert_eq!(16, statistics.bytes());
    assert_eq!(
        b"complete payload",
        std::fs::read(temp.path().join("destination"))
            .expect("the destination should be readable")
            .as_slice(),
    );
}

/// Verifies that rooted copy traverses directory trees without following links.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_directory_copies_regular_descendants() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::create_dir_all(temp.path().join("source/nested"))
        .expect("the source tree should exist");
    std::fs::write(temp.path().join("source/nested/value"), b"value")
        .expect("the source should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let statistics = root
        .copy(&source, &destination, copy::Options::new())
        .expect("the rooted tree should be copied");

    assert_eq!(1, statistics.files());
    assert_eq!(2, statistics.directories());
    assert_eq!(
        b"value",
        std::fs::read(temp.path().join("destination/nested/value"))
            .expect("the destination should be readable")
            .as_slice(),
    );
}

/// Verifies that conservative rooted copy preserves an existing destination.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_rejects_existing_destination() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"source")
        .expect("the source should be written");
    std::fs::write(temp.path().join("destination"), b"destination")
        .expect("the destination should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let error = root
        .copy(&source, &destination, copy::Options::new())
        .expect_err("the destination conflict should be rejected");

    assert_eq!(std::io::ErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        b"destination",
        std::fs::read(temp.path().join("destination"))
            .expect("the destination should remain readable")
            .as_slice(),
    );
}

/// Verifies overwrite and skip policies preserve their distinct contracts.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_applies_explicit_conflict_policies() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"new")
        .expect("the source should be written");
    std::fs::write(temp.path().join("destination"), b"old")
        .expect("the destination should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let skipped = root
        .copy(
            &source,
            &destination,
            copy::Options::new().with_conflict(copy::ConflictPolicy::Skip),
        )
        .expect("skip should keep the destination");
    assert_eq!(1, skipped.skipped());
    assert_eq!(
        b"old",
        std::fs::read(temp.path().join("destination"))
            .unwrap()
            .as_slice()
    );

    let overwritten = root
        .copy(
            &source,
            &destination,
            copy::Options::new().with_conflict(copy::ConflictPolicy::Overwrite),
        )
        .expect("overwrite should replace the destination");
    assert_eq!(1, overwritten.files());
    assert_eq!(1, overwritten.overwritten());
    assert_eq!(
        b"new",
        std::fs::read(temp.path().join("destination"))
            .unwrap()
            .as_slice()
    );
}

/// Verifies type replacement removes the old tree before installing a file.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_replaces_directory_type_conflict() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"file")
        .expect("the source should be written");
    std::fs::create_dir_all(temp.path().join("destination/nested"))
        .expect("the destination tree should exist");
    std::fs::write(temp.path().join("destination/nested/value"), b"old")
        .expect("the old child should exist");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    root.copy(
        &source,
        &destination,
        copy::Options::new()
            .with_type_conflict(copy::TypeConflictPolicy::Replace),
    )
    .expect("the directory should be replaced by a file");

    assert_eq!(
        b"file",
        std::fs::read(temp.path().join("destination"))
            .unwrap()
            .as_slice()
    );
}

/// Verifies invalid self and nested-tree destinations are rejected.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_rejects_self_and_nested_tree_destinations() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::create_dir(temp.path().join("source"))
        .expect("the source should exist");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let nested = rooted::Path::new("source/nested")
        .expect("the nested path should validate");

    assert!(root.copy(&source, &source, copy::Options::new()).is_err());
    assert!(root.copy(&source, &nested, copy::Options::new()).is_err());
}

/// Verifies rooted directory copy uses an explicit stack for deep trees.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_deep_tree_without_recursive_call_stack() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    let mut source_path = temp.path().join("source");
    for _ in 0..128 {
        source_path.push("d");
    }
    std::fs::create_dir_all(&source_path)
        .expect("the deep source should exist");
    std::fs::write(source_path.join("value"), b"deep")
        .expect("the leaf should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let statistics = root
        .copy(&source, &destination, copy::Options::new())
        .expect("the deep tree should copy");

    assert_eq!(1, statistics.files());
    assert_eq!(129, statistics.directories());
}

/// Verifies rooted copy rejects links instead of following them.
#[cfg(unix)]
#[test]
fn test_copy_rejects_symbolic_links() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("target"), b"value")
        .expect("the target should exist");
    symlink("target", temp.path().join("source"))
        .expect("the link should exist");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    assert!(
        root.copy(&source, &destination, copy::Options::new())
            .is_err()
    );
    assert!(
        root.copy(
            &source,
            &destination,
            copy::Options::new().follow_symlinks(),
        )
        .is_err()
    );
}

/// Verifies permission preservation uses the metadata from the opened source.
#[cfg(unix)]
#[test]
fn test_copy_preserves_permissions_when_requested() {
    use std::os::unix::fs::{
        MetadataExt,
        PermissionsExt,
    };

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"value")
        .expect("the source should be written");
    std::fs::set_permissions(
        temp.path().join("source"),
        std::fs::Permissions::from_mode(0o640),
    )
    .expect("the source mode should be set");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    root.copy(
        &source,
        &destination,
        copy::Options::new().preserve_permissions(),
    )
    .expect("the file should copy with permissions");

    assert_eq!(
        0o640,
        std::fs::metadata(temp.path().join("destination"))
            .unwrap()
            .mode()
            & 0o777
    );
}
