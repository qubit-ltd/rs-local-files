// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for rooted `Metadata`.

#[cfg(unix)]
use std::fs;
use std::fs::File;

use qubit_local_files::test_support::internal_contract::EntryKind;
use qubit_local_files::test_support::internal_contract::Metadata;

#[test]
fn test_rooted_metadata_observes_open_file_and_identity() {
    let file = File::open("Cargo.toml").expect("manifest exists");
    let metadata = Metadata::from_open_file(&file).expect("metadata available");
    assert_eq!(metadata.kind(), EntryKind::File);
    assert!(metadata.size() > 0);
    assert!(metadata.is_same_file(&metadata));
    assert!(metadata.accessed_at().is_some());
    assert!(metadata.modified_at().is_some());
    assert!(metadata.created_at().is_some());
    let _ = metadata.permissions();
}

#[cfg(unix)]
#[test]
fn test_rooted_metadata_converts_native_file_metadata() {
    let metadata = fs::metadata("Cargo.toml").expect("manifest exists");
    let rooted = Metadata::from_native(&metadata);
    assert_eq!(rooted.kind(), EntryKind::File);
}
