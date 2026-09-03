// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! In-crate facade workflows that exercise the unit-test build of the library.

use std::fs;
use std::io::Write;
use std::path::Path;

use tempfile::tempdir;

use crate::LocalFileSystem;
use crate::options::LocalCopyOptions;
use crate::options::LocalCreateDirectoryOptions;
use crate::options::LocalDeleteOptions;
use crate::options::LocalListOptions;
use crate::options::LocalPersistOptions;
use crate::options::LocalRenameOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTempFileOptions;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;

/// Exercises the complete rooted facade through the crate's unit-test build,
/// including recursive copy, staging publication, and temporary ownership.
#[test]
fn rooted_facade_workflow_preserves_namespace_and_lifecycle_contracts() {
    let directory = tempdir().expect("temporary root should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let _ = filesystem
        .create_directory_with_options(
            Path::new("source/nested"),
            &LocalCreateDirectoryOptions::new().with_recursive().with_exists_ok(),
        )
        .expect("source directories should be created");
    let mut writer = filesystem
        .open_writer_with_options(
            Path::new("source/nested/payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted writer should open");
    writer.write_all(b"payload").expect("writer should accept bytes");
    let _ = writer.commit().expect("writer should publish");

    let copy = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("copied"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("rooted tree should copy");
    assert_eq!(1, copy.stats().files());
    assert_eq!(
        b"payload",
        filesystem
            .read_prefix(Path::new("copied/nested/payload"), 32)
            .unwrap()
            .as_slice(),
    );

    let entries = filesystem
        .list_with_options(Path::new("copied"), &LocalListOptions::new().with_recursive())
        .expect("rooted walker should open")
        .collect::<Result<Vec<_>, _>>()
        .expect("rooted walker should finish");
    assert_eq!(2, entries.len());
    assert!(filesystem.metadata(Path::new("copied/nested/payload")).is_ok());
    assert!(filesystem.limits_at(Path::new("copied/missing")).is_ok());
    assert!(filesystem.space_at(Path::new("copied/missing")).is_ok());

    let _ = filesystem
        .rename_with_options(
            Path::new("copied/nested/payload"),
            Path::new("copied/nested/renamed"),
            &LocalRenameOptions::new(),
        )
        .expect("rooted file should rename");

    let mut temporary_file = filesystem
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(Path::new("temporary/files"))
                .with_create_parent(),
        )
        .expect("rooted temporary file should be created");
    temporary_file
        .write_all(b"temporary")
        .expect("temporary file should accept bytes");
    let _ = temporary_file
        .persist_with(
            Path::new("published/file"),
            LocalPersistOptions::new().with_create_parent(),
        )
        .expect("temporary file should persist");

    let temporary_directory = filesystem
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_parent(Path::new("temporary/directories"))
                .with_create_parent(),
        )
        .expect("rooted temporary directory should be created");
    let _ = temporary_directory
        .persist_with(
            Path::new("published/directory"),
            LocalPersistOptions::new().with_create_parent(),
        )
        .expect("temporary directory should persist");

    let _ = filesystem
        .delete_directory_with_options(Path::new("source"), &LocalDeleteOptions::new().with_recursive())
        .expect("source tree should be deleted");
}

/// Exercises host copy and temporary-resource paths through the unit-test
/// library so that coverage does not depend on cross-crate inlining.
#[test]
fn host_facade_workflow_covers_copy_and_temporary_resource_paths() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"payload").expect("source fixture should be written");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");

    let copy = filesystem
        .copy_with_options(&source, &target, &LocalCopyOptions::new())
        .expect("host file should copy");
    assert_eq!(1, copy.stats().files());

    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(directory.path()))
        .expect("host temporary file should be created");
    temporary
        .write_all(b"temporary")
        .expect("temporary file should accept bytes");
    temporary.cleanup().expect("temporary file should clean up");

    let mut temporary_directory = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(directory.path()))
        .expect("host temporary directory should be created");
    temporary_directory
        .cleanup()
        .expect("temporary directory should clean up");
}

/// Exercises rooted symbolic-link resolution in the unit-test build.
#[cfg(unix)]
#[test]
fn rooted_facade_resolves_links_and_rejects_virtual_root_escape() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("real")).expect("real directory should be created");
    fs::write(directory.path().join("real/payload"), b"payload").expect("payload should be written");
    symlink("real", directory.path().join("link")).expect("link should be created");
    symlink("../../outside", directory.path().join("escape")).expect("escape link should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    assert_eq!(
        b"payload",
        filesystem
            .read_prefix(Path::new("link/payload"), 32)
            .unwrap()
            .as_slice(),
    );
    assert!(filesystem.open_reader(Path::new("escape")).is_err());
}
