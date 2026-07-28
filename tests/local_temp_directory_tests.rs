// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::path::Path;

use qubit_local_files::{LocalFileSystem, LocalTempDirectoryOptions};
use tempfile::tempdir;

/// Verifies temporary-directory child helpers reject lexical escape shapes.
#[test]
fn test_local_temp_directory_child_helpers_reject_escape_paths() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::create_temp_directory(
        &LocalTempDirectoryOptions::new().with_parent(parent.path()),
    )
    .expect("temporary directory should be created");

    assert!(temporary.child(Path::new("nested/file")).is_err());
    assert!(temporary.child(Path::new(".")).is_err());
    assert!(temporary.child(parent.path()).is_err());
    assert!(temporary.descendant(Path::new("../escape")).is_err());
    assert!(temporary.descendant(Path::new(".")).is_err());
}

/// Verifies Windows path prefixes are rejected as temporary-directory children.
#[cfg(windows)]
#[test]
fn test_local_temp_directory_child_rejects_prefix() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::create_temp_directory(
        &LocalTempDirectoryOptions::new().with_parent(parent.path()),
    )
    .expect("temporary directory should be created");

    assert!(temporary.child(Path::new(r"C:\escape")).is_err());
    assert!(temporary.descendant(Path::new(r"C:\escape")).is_err());
}
