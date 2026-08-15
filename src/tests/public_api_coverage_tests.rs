// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local coverage tests for public filesystem facade delegation.

use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use tempfile::tempdir;

use crate::LocalCopyOptions;
use crate::LocalCreateDirectoryOptions;
use crate::LocalDeleteOptions;
use crate::LocalFileSystem;
use crate::LocalFileSystemScope;
use crate::LocalListOptions;
use crate::LocalReadOptions;
use crate::LocalRenameOptions;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;
use crate::LocalTempDirectoryOptions;
use crate::LocalTempFileOptions;
use crate::LocalWriteMode;
use crate::LocalWriteOptions;

/// Verifies the public facade delegates every ordinary host operation through
/// the library crate, retaining the expected filesystem effects.
#[test]
fn test_public_host_facade_delegates_ordinary_operations() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = LocalFileSystem::host();
    assert_eq!(LocalFileSystemScope::Host, filesystem.scope());
    assert!(filesystem.diagnostic_root().is_none());
    let _ = filesystem.protocols();
    let _ = filesystem.limits();
    let _ = filesystem
        .limits_at(directory.path())
        .expect("host limits should be available");
    let _ = filesystem
        .space_at(directory.path())
        .expect("host space should be available");

    let source = directory.path().join("source");
    let copied = directory.path().join("copied");
    let renamed = directory.path().join("renamed");
    fs::write(&source, b"payload").expect("source fixture should be written");
    assert_eq!(
        7,
        filesystem
            .metadata(&source)
            .expect("source metadata should be available")
            .len()
    );
    let mut reader = filesystem
        .open_reader(&source, &LocalReadOptions::new())
        .expect("source reader should open");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("source reader should read");
    assert_eq!("payload", content);
    assert_eq!(
        b"pay",
        filesystem
            .read_prefix(&source, &LocalReadOptions::new(), 3)
            .expect("prefix should be readable")
            .as_slice()
    );

    let _ = filesystem
        .copy(&source, &copied, &LocalCopyOptions::new())
        .expect("file should copy");
    let _ = filesystem
        .rename(&copied, &renamed, &LocalRenameOptions::new())
        .expect("file should rename");
    let mut writer = filesystem
        .open_writer(&renamed, &LocalWriteOptions::new(LocalWriteMode::Append))
        .expect("append writer should open");
    writer.write_all(b"!").expect("append writer should write");
    let _ = writer.commit().expect("append writer should commit");

    let created = directory.path().join("created");
    let _ = filesystem
        .create_directory(&created, &LocalCreateDirectoryOptions::new())
        .expect("directory should be created");
    assert_eq!(
        3,
        filesystem
            .list(directory.path(), &LocalListOptions::new())
            .expect("directory listing should open")
            .collect::<Result<Vec<_>, _>>()
            .expect("directory listing should complete")
            .len()
    );
    let _ = filesystem
        .delete_directory(&created, &LocalDeleteOptions::new())
        .expect("directory should be deleted");
    let _ = filesystem
        .delete_file(&renamed, &LocalDeleteOptions::new())
        .expect("file should be deleted");

    let mut temporary_file = filesystem
        .create_temp_file(
            &LocalTempFileOptions::new().with_parent(directory.path()),
        )
        .expect("temporary file should be created");
    temporary_file.close();
    temporary_file
        .cleanup()
        .expect("temporary file should clean up");
    let mut temporary_directory = filesystem
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(directory.path()),
        )
        .expect("temporary directory should be created");
    temporary_directory
        .cleanup()
        .expect("temporary directory should clean up");
}

/// Verifies rooted facade entry points accept relative descendants while
/// preserving their opened authority and diagnostic root.
#[test]
fn test_public_rooted_facade_delegates_relative_operations() {
    let directory = tempdir().expect("temporary root should be created");
    let filesystem = LocalFileSystem::rooted(directory.path())
        .expect("rooted filesystem should open");
    assert_eq!(LocalFileSystemScope::Rooted, filesystem.scope());
    assert_eq!(Some(directory.path()), filesystem.diagnostic_root());
    let _ = filesystem.protocols();
    let _ = filesystem.limits();
    let _ = filesystem
        .limits_at(Path::new("missing/child"))
        .expect("rooted limits should be available");
    let _ = filesystem
        .space_at(Path::new("missing/child"))
        .expect("rooted space should be available");

    let _ = filesystem
        .create_directory(
            Path::new("nested"),
            &LocalCreateDirectoryOptions::new(),
        )
        .expect("rooted directory should be created");
    let mut writer = filesystem
        .open_writer(
            Path::new("nested/source"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("rooted writer should open");
    writer
        .write_all(b"payload")
        .expect("rooted writer should write");
    let _ = writer.commit().expect("rooted writer should commit");
    let _ = filesystem
        .copy(
            Path::new("nested/source"),
            Path::new("nested/copied"),
            &LocalCopyOptions::new(),
        )
        .expect("rooted file should copy");
    let _ = filesystem
        .rename(
            Path::new("nested/copied"),
            Path::new("nested/renamed"),
            &LocalRenameOptions::new(),
        )
        .expect("rooted file should rename");
    let _ = filesystem
        .delete_file(Path::new("nested/renamed"), &LocalDeleteOptions::new())
        .expect("rooted file should be deleted");
}

/// Verifies resource-limit diagnostics retain every supplied budget fact.
#[test]
fn test_resource_limit_error_retains_budget_facts() {
    let error =
        LocalResourceLimitError::new(LocalResourceKind::OpenDirectory, 8, 2, 3);

    assert_eq!(LocalResourceKind::OpenDirectory, error.resource());
    assert_eq!(8, error.limit());
    assert_eq!(2, error.remaining());
    assert_eq!(3, error.requested());
}
