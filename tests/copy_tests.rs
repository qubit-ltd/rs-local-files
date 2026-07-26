// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::copy;

/// Verifies copy options are exposed through the responsibility module.
#[test]
fn test_copy_options_are_available() {
    let _ = copy::Options::default();
}

/// Verifies recursive copying reports the copied tree.
#[test]
fn test_copy_directory_copies_regular_files() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::create_dir(&source).expect("the source should be created");
    std::fs::write(source.join("payload"), b"payload")
        .expect("the source fixture should be written");

    let statistics =
        copy::directory(&source, &destination, copy::Options::default())
            .expect("the source tree should be copied");

    assert_eq!(1, statistics.files());
    assert_eq!(7, statistics.bytes());
    assert_eq!(
        b"payload",
        std::fs::read(destination.join("payload"))
            .unwrap()
            .as_slice()
    );
}
