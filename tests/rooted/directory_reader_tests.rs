// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_local_files::{
    LocalFileSystem,
    LocalListOptions,
};
use tempfile::tempdir;

/// Verifies Rooted listing lazily yields authorized direct children and then
/// terminates.
#[test]
fn test_rooted_directory_reader_yields_children_lazily() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("first"), b"first")
        .expect("first fixture should be written");
    fs::write(directory.path().join("second"), b"second")
        .expect("second fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path())
        .expect("Rooted filesystem should open");
    let walker = filesystem
        .list(Path::new(""), &LocalListOptions::new())
        .expect("Rooted directory walker should open");
    let mut names = walker
        .map(|entry| {
            entry
                .expect("Rooted child should be readable")
                .relative_path()
                .to_path_buf()
        })
        .collect::<Vec<_>>();
    names.sort();

    assert_eq!(vec![Path::new("first"), Path::new("second")], names);
}
