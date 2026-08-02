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
    LocalFileKind,
    LocalFileSystem,
    LocalFileSystemScope,
    metadata,
};
use tempfile::tempdir;

/// Verifies Host methods and convenience functions inspect the same native
/// namespace.
#[test]
fn test_local_file_system_host_matches_convenience_functions() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");
    let filesystem = LocalFileSystem::host();

    assert_eq!(LocalFileSystemScope::Host, filesystem.scope());
    assert_eq!(
        LocalFileKind::File,
        filesystem
            .metadata(&path)
            .expect("Host instance should inspect the fixture",)
            .kind()
    );
    let convenience = metadata(&path)
        .expect("Host convenience function should inspect the fixture");
    let instance = filesystem
        .metadata(&path)
        .expect("Host instance should inspect the fixture");
    assert_eq!(convenience.kind(), instance.kind());
    assert_eq!(convenience.len(), instance.len());
}

/// Verifies Rooted scope and its separate diagnostic root accessor.
#[test]
fn test_local_file_system_rooted_reports_scope_and_reads_relative_path() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("payload"), b"payload")
        .expect("fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path())
        .expect("Rooted filesystem should open");

    assert_eq!(
        LocalFileSystemScope::Rooted,
        filesystem.scope(),
    );
    assert_eq!(Some(directory.path()), filesystem.diagnostic_root());
    assert_eq!(
        LocalFileKind::File,
        filesystem
            .metadata(Path::new("payload"))
            .expect("Rooted instance should inspect a relative fixture")
            .kind(),
    );
}
