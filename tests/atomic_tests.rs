// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Write;

use qubit_local_files::atomic;

/// Verifies atomic options are exposed through the responsibility module.
#[test]
fn test_atomic_options_are_available() {
    let _ = atomic::Options::new();
}

/// Verifies both atomic entry points install complete file contents.
#[test]
fn test_atomic_begin_entry_points_install_files() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let nested = directory.path().join("nested/default.txt");
    let mut writer = atomic::begin(&nested)
        .expect("default atomic writing should create parents");
    writer
        .write_all(b"default")
        .expect("staged content should be written");
    writer.commit().expect("staged content should be installed");

    let explicit = directory.path().join("explicit.txt");
    let mut writer = atomic::begin_with(&explicit, atomic::Options::new())
        .expect("explicit atomic writing should start");
    writer
        .write_all(b"explicit")
        .expect("staged content should be written");
    writer.commit().expect("staged content should be installed");

    assert_eq!(b"default", std::fs::read(nested).unwrap().as_slice());
    assert_eq!(b"explicit", std::fs::read(explicit).unwrap().as_slice());
}
