// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Verifies local rename moves a file.
#[test]
fn test_rename_moves_file() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    std::fs::write(&source, b"value").expect("the source should be written");
    qubit_local_files::rename::move_path(&source, &target)
        .expect("the file should move");
    assert!(target.exists());
}

/// Verifies no-replace rename preserves both entries on a destination
/// conflict.
#[test]
fn test_rename_without_replacing_preserves_existing_destination() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    std::fs::write(&source, b"source").expect("the source should be written");
    std::fs::write(&target, b"target").expect("the target should be written");

    let error = qubit_local_files::rename::move_path_without_replacing(
        &source, &target,
    )
    .expect_err("the destination conflict should be rejected");

    assert_eq!(std::io::ErrorKind::AlreadyExists, error.kind());
    assert_eq!(b"source", std::fs::read(&source).unwrap().as_slice());
    assert_eq!(b"target", std::fs::read(&target).unwrap().as_slice());
}

/// Verifies no-replace rename supports directory entries.
#[test]
fn test_rename_directory_without_replacing() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    std::fs::create_dir(&source).expect("the source should be created");

    qubit_local_files::rename::move_path_without_replacing(&source, &target)
        .expect("the directory should move");

    assert!(!source.exists());
    assert!(target.is_dir());
}

/// Verifies no-replace rename reports a missing source.
#[test]
fn test_rename_without_replacing_rejects_missing_source() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("missing");
    let target = directory.path().join("target");

    let error = qubit_local_files::rename::move_path_without_replacing(
        &source, &target,
    )
    .expect_err("the missing source should be rejected");

    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
}
