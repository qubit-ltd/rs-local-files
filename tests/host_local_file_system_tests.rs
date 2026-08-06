// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    io::{Read, Write},
};

use qubit_local_files::{
    LocalCopyOptions, LocalCreateDirectoryOptions, LocalDeleteOptions, LocalFileKind,
    LocalFileSystem, LocalListOptions, LocalReadOptions, LocalRenameOptions,
    LocalTempDirectoryOptions, LocalTempFileOptions, LocalWriteMode, LocalWriteOptions,
};
use tempfile::tempdir;

/// Verifies the Host filesystem inspects the process-visible native namespace.
#[test]
fn test_host_local_file_system_inspects_native_namespace() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");

    let configured = LocalFileSystem::host()
        .metadata(&path)
        .expect("configured Host filesystem should inspect the fixture");

    assert_eq!(LocalFileKind::File, configured.kind());
    assert_eq!(configured.len(), b"payload".len() as u64);
}

/// Verifies the complete Host workflow through one configured filesystem.
#[test]
fn test_host_local_file_system_workflow() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = LocalFileSystem::host();
    let tree = directory.path().join("tree");
    let _ = filesystem
        .create_directory(&tree, &LocalCreateDirectoryOptions::new().with_recursive())
        .expect("directory should be created");

    let source = tree.join("source");
    let mut writer = filesystem
        .open_writer(&source, &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("writer should open");
    writer
        .write_all(b"payload")
        .expect("payload should be written");
    let _ = writer.commit().expect("payload should be committed");

    let mut payload = Vec::new();
    filesystem
        .open_reader(&source, &LocalReadOptions::new())
        .expect("reader should open")
        .read_to_end(&mut payload)
        .expect("payload should be read");
    assert_eq!(b"payload", payload.as_slice());
    assert_eq!(
        1,
        filesystem
            .list(&tree, &LocalListOptions::new())
            .expect("tree should list")
            .count()
    );

    let copied = tree.join("copied");
    let _ = filesystem
        .copy(&source, &copied, &LocalCopyOptions::new())
        .expect("file should be copied");
    let renamed = tree.join("renamed");
    let _ = filesystem
        .rename(&copied, &renamed, &LocalRenameOptions::new())
        .expect("file should be renamed");
    let _ = filesystem
        .delete_file(&renamed, &LocalDeleteOptions::new())
        .expect("renamed file should be deleted");

    let temp_file = filesystem
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("temporary file should be created");
    let temp_directory = filesystem
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary directory should be created");
    assert!(temp_file.path().exists());
    assert!(temp_directory.path().exists());

    let _ = filesystem
        .delete_file(&source, &LocalDeleteOptions::new())
        .expect("source file should be deleted");
    let _ = filesystem
        .delete_directory(&tree, &LocalDeleteOptions::new())
        .expect("tree should be deleted");
}

/// Verifies the Host prefix operation through the configured filesystem.
#[test]
fn test_host_read_prefix() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");

    let filesystem = LocalFileSystem::host();
    assert_eq!(
        b"pay".as_slice(),
        filesystem
            .read_prefix(&path, &LocalReadOptions::new(), 3)
            .expect("prefix should be readable")
            .as_slice()
    );
}
