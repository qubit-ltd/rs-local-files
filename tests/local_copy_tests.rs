// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;

use qubit_local_files::{
    LocalCopyConflictPolicy,
    LocalCopyMethod,
    LocalCopyOptions,
    LocalDurabilityRequirement,
    LocalFileErrorKind,
    LocalFileSystem,
};
use tempfile::tempdir;

/// Verifies that one copy entry handles both regular files and directory trees.
#[test]
fn test_local_file_system_copy_unifies_file_and_directory_copy() {
    let directory = tempdir().expect("temporary directory should be created");
    let source_file = directory.path().join("source.txt");
    let target_file = directory.path().join("target.txt");
    fs::write(&source_file, b"file").expect("source file should be written");

    let file_outcome = LocalFileSystem::copy(
        &source_file,
        &target_file,
        &LocalCopyOptions::new(),
    )
    .expect("file copy should succeed");
    assert_eq!(LocalCopyMethod::StagedFile, file_outcome.method());
    assert_eq!(1, file_outcome.stats().files());
    assert_eq!(4, file_outcome.stats().bytes());

    let source_directory = directory.path().join("source-directory");
    let target_directory = directory.path().join("target-directory");
    fs::create_dir(&source_directory)
        .expect("source directory should be created");
    fs::write(source_directory.join("child"), b"tree")
        .expect("child should be written");

    let tree_outcome = LocalFileSystem::copy(
        &source_directory,
        &target_directory,
        &LocalCopyOptions::new(),
    )
    .expect("directory copy should succeed");
    assert_eq!(LocalCopyMethod::Recursive, tree_outcome.method());
    assert_eq!(1, tree_outcome.stats().files());
    assert_eq!(
        b"tree",
        fs::read(target_directory.join("child"))
            .expect("child should copy")
            .as_slice()
    );
}

/// Verifies source and hard-link alias detection before destructive overwrite.
#[cfg(unix)]
#[test]
fn test_local_file_system_copy_rejects_hard_link_alias() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let alias = directory.path().join("alias");
    fs::write(&source, b"payload").expect("source should be written");
    fs::hard_link(&source, &alias).expect("hard-link alias should be created");

    let error = LocalFileSystem::copy(
        &source,
        &alias,
        &LocalCopyOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite),
    )
    .expect_err("copying onto a hard-link alias must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidInput, error.kind());
    assert_eq!(
        b"payload",
        fs::read(&source)
            .expect("source should remain intact")
            .as_slice()
    );
}

/// Verifies overwrite replaces a target symlink entry rather than its referent.
#[cfg(unix)]
#[test]
fn test_local_file_system_copy_overwrite_replaces_target_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let referent = directory.path().join("referent");
    let target = directory.path().join("target");
    fs::write(&source, b"new").expect("source should be written");
    fs::write(&referent, b"old").expect("referent should be written");
    symlink(&referent, &target).expect("target symlink should be created");

    let _outcome = LocalFileSystem::copy(
        &source,
        &target,
        &LocalCopyOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite),
    )
    .expect("overwrite should replace the target entry");

    assert!(
        !fs::symlink_metadata(&target)
            .expect("target should exist")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        b"new",
        fs::read(&target)
            .expect("target should contain copied bytes")
            .as_slice()
    );
    assert_eq!(
        b"old",
        fs::read(&referent)
            .expect("referent should remain unchanged")
            .as_slice()
    );
}

/// Verifies preferred file-copy durability reports a parent-sync downgrade.
#[cfg(unix)]
#[test]
fn test_local_copy_preferred_durability_reports_downgrade() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let parent = directory.path().join("parent");
    let target = parent.join("target");
    fs::write(&source, b"payload").expect("source should be written");
    fs::create_dir(&parent).expect("target parent should be created");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o300))
        .expect("target parent should reject directory open");

    let outcome = LocalFileSystem::copy(
        &source,
        &target,
        &LocalCopyOptions::new()
            .with_durability(LocalDurabilityRequirement::Preferred),
    )
    .expect("preferred copy durability may report a downgrade");

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
        .expect("target parent permissions should be restored");
    assert!(!outcome.durable());
    assert_eq!(
        b"payload",
        fs::read(&target)
            .expect("copied target should remain")
            .as_slice(),
    );
}
