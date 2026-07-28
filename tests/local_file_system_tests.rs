// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    io::Read,
};

use qubit_local_files::{
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalDurabilityRequirement,
    LocalFileErrorKind,
    LocalFileSystem,
    LocalMutationState,
    LocalReadOptions,
    LocalRenameOptions,
};
use tempfile::tempdir;

/// Verifies explicit parent creation and its structured outcome.
#[test]
fn test_local_file_system_create_directory_reports_created_entries() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("one/two");

    let outcome = LocalFileSystem::create_directory(
        &target,
        &LocalCreateDirectoryOptions::new().with_recursive(),
    )
    .expect("recursive directory creation should succeed");

    assert!(outcome.created());
    assert!(target.is_dir());
}

/// Verifies that opening a reader rejects directories and reads regular files.
#[test]
fn test_local_file_system_open_reader_requires_regular_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("payload");
    fs::write(&file, b"content").expect("fixture should be written");

    let mut reader =
        LocalFileSystem::open_reader(&file, &LocalReadOptions::new())
            .expect("regular file should open");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("reader should return fixture content");
    assert_eq!("content", content);

    let error = LocalFileSystem::open_reader(
        directory.path(),
        &LocalReadOptions::new(),
    )
    .expect_err("directory must not be exposed as a file reader");
    assert_eq!(LocalFileErrorKind::TypeConflict, error.kind());
}

/// Verifies no-replace rename and explicit overwrite behavior.
#[test]
fn test_local_file_system_rename_respects_overwrite_policy() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"new").expect("source fixture should be written");
    fs::write(&target, b"old").expect("target fixture should be written");

    let error =
        LocalFileSystem::rename(&source, &target, &LocalRenameOptions::new())
            .expect_err("default rename must not replace an entry");
    assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        b"old".as_slice(),
        fs::read(&target)
            .expect("target should remain readable")
            .as_slice(),
    );

    let outcome = LocalFileSystem::rename(
        &source,
        &target,
        &LocalRenameOptions::new().with_overwrite(),
    )
    .expect("explicit overwrite should replace the target entry");
    assert!(outcome.atomic());
    assert_eq!(
        b"new".as_slice(),
        fs::read(&target)
            .expect("target should be replaced")
            .as_slice(),
    );
}

/// Verifies rename durability distinguishes preferred downgrade from required
/// partial success.
#[cfg(unix)]
#[test]
fn test_local_file_system_rename_reports_parent_sync_result() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    for requirement in [
        LocalDurabilityRequirement::Preferred,
        LocalDurabilityRequirement::Required,
    ] {
        let source = directory.path().join(format!("source-{requirement:?}"));
        let parent = directory.path().join(format!("parent-{requirement:?}"));
        let target = parent.join("target");
        fs::write(&source, b"payload").expect("source should be written");
        fs::create_dir(&parent).expect("target parent should be created");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o300))
            .expect("target parent should reject directory open");
        let result = LocalFileSystem::rename(
            &source,
            &target,
            &LocalRenameOptions::new().with_durability(requirement),
        );
        match requirement {
            LocalDurabilityRequirement::Preferred => {
                let outcome = result
                    .expect("preferred durability may report a downgrade");
                assert!(!outcome.durable());
            }
            LocalDurabilityRequirement::Required => {
                let error =
                    result.expect_err("required durability must report failure");
                assert_eq!(
                    LocalFileErrorKind::PublicationIncomplete,
                    error.kind(),
                );
                assert_eq!(
                    Some(LocalMutationState::Published),
                    error.mutation_state(),
                );
            }
            LocalDurabilityRequirement::NotRequired => unreachable!(),
        }
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
            .expect("target parent permissions should be restored");
        assert_eq!(
            b"payload",
            fs::read(&target)
                .expect("renamed target should remain")
                .as_slice(),
        );
    }
}

/// Verifies file and recursive-directory deletion semantics.
#[test]
fn test_local_file_system_delete_uses_explicit_directory_recursion() {
    let directory = tempdir().expect("temporary directory should be created");
    let tree = directory.path().join("tree");
    fs::create_dir(&tree).expect("tree root should be created");
    fs::write(tree.join("child"), b"x").expect("tree child should be written");

    let error =
        LocalFileSystem::delete_directory(&tree, &LocalDeleteOptions::new())
            .expect_err(
                "non-recursive deletion should reject a non-empty directory",
            );
    assert!(matches!(
        error.kind(),
        LocalFileErrorKind::Io | LocalFileErrorKind::TypeConflict
    ));

    let outcome = LocalFileSystem::delete_directory(
        &tree,
        &LocalDeleteOptions::new().with_recursive(),
    )
    .expect("recursive deletion should remove the tree");
    assert!(outcome.deleted());
    assert!(!tree.exists());
}
