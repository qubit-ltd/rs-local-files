// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    path::PathBuf,
};

use qubit_local_files::{
    LocalAtomicityRequirement,
    LocalCopyConflictPolicy,
    LocalCopyFailureState,
    LocalCopyMethod,
    LocalCopyOptions,
    LocalCopyStats,
    LocalCopyTypeConflictPolicy,
    LocalFileErrorKind,
    LocalFileSystem,
};
#[cfg(unix)]
use qubit_local_files::{
    LocalDurabilityRequirement,
    LocalMetadataPreservePolicy,
};
use tempfile::tempdir;

/// Verifies nested type conflicts honor `Skip` without traversing skipped
/// source subtrees or modifying destination entries.
#[test]
fn test_copy_tree_skips_nested_type_conflicts() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir_all(source.join("directory-to-file"))
        .expect("source directories should be created");
    fs::write(source.join("file-to-directory"), b"source-file")
        .expect("source file should be written");
    fs::write(source.join("directory-to-file/hidden"), b"hidden")
        .expect("source subtree should be written");
    fs::create_dir_all(target.join("file-to-directory"))
        .expect("target directories should be created");
    fs::write(target.join("file-to-directory/kept"), b"kept")
        .expect("target child should be written");
    fs::write(target.join("directory-to-file"), b"target-file")
        .expect("target file should be written");

    let outcome = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Skip),
        )
        .expect("nested type conflicts should be skipped");

    assert_eq!(2, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().files());
    assert_eq!(
        b"kept",
        fs::read(target.join("file-to-directory/kept"))
            .expect("target directory should remain")
            .as_slice(),
    );
    assert_eq!(
        b"target-file",
        fs::read(target.join("directory-to-file"))
            .expect("target file should remain")
            .as_slice(),
    );
}

/// Creates a process-specific path that is absent before each test use.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "qubit-local-files-copy-{name}-{}",
        std::process::id()
    ))
}

/// Verifies copy preflight failures preserve an unchanged destination state.
#[test]
fn test_copy_failure_preserves_unchanged_state() {
    let source = temp_path("missing-source");
    let target = temp_path("missing-target");

    let failure = LocalFileSystem::host()
        .copy(&source, &target, &LocalCopyOptions::default())
        .expect_err("missing source must fail");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(&LocalCopyStats::default(), failure.partial_stats());
    assert!(!target.exists());
}

/// Verifies that one copy entry handles both regular files and directory trees.
#[test]
fn test_local_file_system_copy_unifies_file_and_directory_copy() {
    let directory = tempdir().expect("temporary directory should be created");
    let source_file = directory.path().join("source.txt");
    let target_file = directory.path().join("target.txt");
    fs::write(&source_file, b"file").expect("source file should be written");

    let file_outcome = LocalFileSystem::host()
        .copy(&source_file, &target_file, &LocalCopyOptions::new())
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

    let tree_outcome = LocalFileSystem::host()
        .copy(
            &source_directory,
            &target_directory,
            &LocalCopyOptions::new().with_tree_source(),
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

/// Verifies automatic source selection recognizes directory trees.
#[test]
fn test_local_file_system_copy_auto_detects_directory_sources() {
    let directory = tempdir().expect("temporary directory should exist");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir(&source).expect("source directory should be created");

    let outcome = LocalFileSystem::host()
        .copy(&source, &target, &LocalCopyOptions::new())
        .expect("automatic source selection must copy a directory tree");
    assert_eq!(LocalCopyMethod::Recursive, outcome.method());
    assert!(target.is_dir());
}

/// Verifies required atomic replacement rejects a file-to-directory conflict.
#[test]
fn test_local_file_system_copy_rejects_required_directory_replacement() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"payload").expect("source file should be written");
    fs::create_dir(&target).expect("target directory should be created");

    let error = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_atomicity(LocalAtomicityRequirement::Required)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
        )
        .expect_err("required atomic directory replacement must be rejected");
    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.error().kind());
}

/// Verifies file conflicts honor fail and overwrite policies, while type
/// conflicts honor the default fail policy.
#[test]
fn test_local_file_system_copy_conflict_policy_matrix() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"source").expect("source should be written");
    fs::write(&target, b"target").expect("target should be written");

    LocalFileSystem::host()
        .copy(&source, &target, &LocalCopyOptions::new())
        .expect_err("existing file should fail under the default policy");
    let outcome = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite),
        )
        .expect("overwrite policy should replace an existing file");
    assert_eq!(1, outcome.stats().files());
    assert_eq!(b"source", fs::read(&target).unwrap().as_slice());

    fs::remove_file(&target).expect("file target should be removed");
    fs::create_dir(&target).expect("directory target should be created");
    LocalFileSystem::host()
        .copy(&source, &target, &LocalCopyOptions::new())
        .expect_err("file-to-directory conflict should fail by default");
}

/// Verifies recursive copy reads and applies source directory permissions.
#[cfg(unix)]
#[test]
fn test_recursive_copy_preserves_directory_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir(&source).expect("source directory should be created");
    fs::write(source.join("payload"), b"payload")
        .expect("source payload should be written");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o750))
        .expect("source permissions should be configured");

    let outcome = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_metadata_preservation(
                    LocalMetadataPreservePolicy::Permissions,
                ),
        )
        .expect("recursive copy should preserve directory permissions");
    assert_eq!(1, outcome.stats().files());
    assert_eq!(
        0o750,
        fs::metadata(&target).unwrap().permissions().mode() & 0o7777
    );
}

/// Verifies recursive copies preserve nested final symlink entries instead of
/// dereferencing file links.
#[cfg(unix)]
#[test]
fn test_recursive_copy_preserves_nested_symlink_entry() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should exist");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir(&source).expect("source directory should be created");
    fs::write(source.join("referent"), b"payload")
        .expect("referent should be written");
    symlink("referent", source.join("link"))
        .expect("source symlink should be created");

    let _ = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("recursive copy should preserve the nested symlink");

    assert_eq!(
        PathBuf::from("referent"),
        fs::read_link(target.join("link"))
            .expect("target symlink should exist")
    );
    assert_eq!(
        b"payload",
        fs::read(target.join("referent"))
            .expect("referent should be copied")
            .as_slice()
    );
}

/// Verifies source and hard-link alias detection before destructive overwrite.
#[cfg(any(unix, windows))]
#[test]
fn test_local_file_system_copy_rejects_hard_link_alias() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let alias = directory.path().join("alias");
    fs::write(&source, b"payload").expect("source should be written");
    fs::hard_link(&source, &alias).expect("hard-link alias should be created");

    let error = LocalFileSystem::host()
        .copy(
            &source,
            &alias,
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite),
        )
        .expect_err("copying onto a hard-link alias must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.error().kind());
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

    let outcome = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite),
        )
        .expect("overwrite should replace the target entry");

    assert_eq!(1, outcome.stats().overwritten());

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

/// Verifies a dangling target symlink is still an existing type conflict.
#[cfg(unix)]
#[test]
fn test_local_file_system_copy_skips_dangling_target_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir(&source).expect("source directory should be created");
    symlink("missing", &target)
        .expect("dangling target symlink should be created");

    let outcome = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_type_conflict(LocalCopyTypeConflictPolicy::Skip),
        )
        .expect("the type conflict should be skipped");

    assert_eq!(1, outcome.stats().skipped());
    let metadata =
        fs::symlink_metadata(&target).expect("target symlink should remain");
    assert!(metadata.file_type().is_symlink());
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

    let outcome = LocalFileSystem::host()
        .copy(
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
