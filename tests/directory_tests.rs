// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Verifies directory creation is idempotent.
#[test]
fn test_directory_create_all_is_idempotent() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    qubit_local_files::directory::create_all(directory.path())
        .expect("an existing directory should be accepted");
}

/// Verifies single-directory creation rejects an existing destination.
#[test]
fn test_directory_create_requires_a_new_entry() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let child = directory.path().join("child");

    qubit_local_files::directory::create(&child)
        .expect("a missing child directory should be created");
    let error = qubit_local_files::directory::create(&child)
        .expect_err("an existing child directory should be rejected");

    assert_eq!(std::io::ErrorKind::AlreadyExists, error.kind());
}

/// Verifies listing, sizing, parent creation, and clearing share one facade.
#[test]
fn test_directory_lifecycle_operations() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let child = directory.path().join("nested/payload");
    qubit_local_files::directory::create_parent(&child)
        .expect("the parent should be created");
    std::fs::write(&child, b"payload").expect("the fixture should be written");

    assert_eq!(
        1,
        qubit_local_files::directory::read(directory.path())
            .expect("the directory should be readable")
            .count(),
    );
    assert_eq!(
        7,
        qubit_local_files::directory::size(directory.path())
            .expect("the directory should be sized"),
    );

    qubit_local_files::directory::clear(directory.path())
        .expect("the directory should be cleared");
    assert_eq!(0, std::fs::read_dir(directory.path()).unwrap().count());
}
