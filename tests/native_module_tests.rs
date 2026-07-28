// =============================================================================

#![cfg(coverage)]
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Write;

use qubit_local_files::{
    atomic,
    directory,
    metadata,
    remove,
    rename,
    temp,
};

/// Verifies native operations are exposed by responsibility-focused modules.
#[test]
fn test_native_modules_cover_common_file_lifecycle() {
    let directory_handle =
        temp::TempDir::new().expect("a temporary directory should be created");
    let source = directory_handle.path().join("source.txt");
    let target = directory_handle.path().join("target.txt");
    let moved = directory_handle.path().join("moved.txt");

    let mut writer =
        atomic::begin(&source).expect("an atomic writer should be created");
    writer
        .write_all(b"payload")
        .expect("the staging writer should accept bytes");
    writer
        .commit()
        .expect("the staging file should be committed");

    assert!(
        metadata::exists(&source)
            .expect("source existence should be observable")
    );
    assert_eq!(
        7,
        metadata::read(&source)
            .expect("source metadata should be readable")
            .len()
    );
    std::fs::copy(&source, &target).expect("the fixture should be copied");
    rename::move_path(&target, &moved)
        .expect("the copied file should be renamed");
    remove::file(&moved).expect("the renamed file should be removed");
    assert_eq!(
        1,
        directory::read(directory_handle.path())
            .expect("the temporary directory should be listed")
            .count()
    );
}
