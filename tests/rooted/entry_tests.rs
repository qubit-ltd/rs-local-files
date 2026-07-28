// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::ffi::OsStr;

#[cfg(unix)]
use qubit_local_files::rooted::{EntryKind, Root};

/// Verifies rooted directory entries expose native names and no-follow
/// metadata.
#[cfg(unix)]
#[test]
fn test_rooted_entry_exposes_name_and_metadata() {
    let temporary_directory = tempfile::tempdir().expect("a temporary root should be created");
    std::fs::write(temporary_directory.path().join("value.txt"), b"value")
        .expect("the fixture should be written");
    let root = Root::open(temporary_directory.path()).expect("the root should open");

    let entries = root
        .read_root_dir()
        .expect("the root directory should be listed");

    assert_eq!(1, entries.len());
    assert_eq!(OsStr::new("value.txt"), entries[0].name());
    assert_eq!(EntryKind::File, entries[0].metadata().kind());
    assert_eq!(5, entries[0].metadata().size());
}
