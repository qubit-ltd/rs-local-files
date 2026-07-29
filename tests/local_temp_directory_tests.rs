// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::path::Path;

use qubit_local_files::{LocalFileSystem, LocalPersistOptions, LocalTempDirectoryOptions};
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

/// Verifies directory persistence honors the requested replacement policy.
#[test]
fn test_local_temp_directory_persist_with_overwrite_replaces_empty_destination() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::create_temp_directory(
        &LocalTempDirectoryOptions::new().with_parent(parent.path()),
    )
    .expect("temporary directory should be created");
    let target = parent.path().join("published");
    std::fs::create_dir(&target).expect("empty destination should be created");

    let persisted = temporary
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect("overwrite persistence should replace an empty directory");

    assert_eq!(persisted, target);
    assert!(persisted.is_dir());
}

/// Verifies a failed directory cleanup leaves the guard available for retry.
#[test]
fn test_local_temp_directory_cleanup_failure_retains_resource_for_retry() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::create_temp_directory(
        &LocalTempDirectoryOptions::new().with_parent(parent.path()),
    )
    .expect("temporary directory should be created");
    let path = temporary.path().to_path_buf();
    std::fs::remove_dir(&path).expect("fixture should remove the temporary directory");

    assert!(temporary.cleanup().is_err());
    std::fs::create_dir(&path).expect("fixture should restore the temporary directory");
    temporary
        .cleanup()
        .expect("the retained temporary directory should support cleanup retry");
    assert!(!path.exists());
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
