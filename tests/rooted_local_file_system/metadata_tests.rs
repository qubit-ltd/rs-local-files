// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted metadata-operation regression tests.

use std::fs;

use qubit_local_files::LocalFileKind;
use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;

/// Verifies the split Rooted metadata module retains authority-relative reads.
#[test]
fn test_rooted_metadata_operation_reads_relative_file() {
    let directory = tempdir().expect("metadata test directory should exist");
    fs::write(directory.path().join("payload"), b"payload")
        .expect("metadata test payload should be written");
    let filesystem = LocalFileSystem::rooted(directory.path())
        .expect("rooted filesystem should open");

    let metadata = filesystem
        .metadata(std::path::Path::new("payload"))
        .expect("rooted metadata should read relative file");
    assert_eq!(LocalFileKind::File, metadata.kind());
}
