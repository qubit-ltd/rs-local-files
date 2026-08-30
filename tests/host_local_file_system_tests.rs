// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::io::Read;
use std::io::Write;

use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalCreateDirectoryOptions;
use qubit_local_files::LocalDeleteOptions;
use qubit_local_files::LocalFileKind;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalListOptions;
use qubit_local_files::LocalReadOptions;
use qubit_local_files::LocalRenameOptions;
use qubit_local_files::LocalTempDirectoryOptions;
use qubit_local_files::LocalTempFileOptions;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;
use tempfile::tempdir;

/// Verifies the Host filesystem inspects the process-visible native namespace.
#[test]
fn test_host_local_file_system_inspects_native_namespace() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");

    let configured = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&path)
        .expect("configured Host filesystem should inspect the fixture");

    assert_eq!(LocalFileKind::File, configured.kind());
    assert_eq!(configured.len(), b"payload".len() as u64);
}

/// Verifies the complete Host workflow through one configured filesystem.
#[test]
fn test_host_local_file_system_workflow() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let tree = directory.path().join("tree");
    let _ = filesystem
        .create_directory_with_options(&tree, &LocalCreateDirectoryOptions::new().with_recursive())
        .expect("directory should be created");

    let source = tree.join("source");
    let mut writer = filesystem
        .open_writer_with_options(&source, &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("writer should open");
    writer.write_all(b"payload").expect("payload should be written");
    let _ = writer.commit().expect("payload should be committed");

    let mut payload = Vec::new();
    filesystem
        .open_reader_with_options(&source, &LocalReadOptions::new())
        .expect("reader should open")
        .read_to_end(&mut payload)
        .expect("payload should be read");
    assert_eq!(b"payload", payload.as_slice());
    assert_eq!(
        1,
        filesystem
            .list_with_options(&tree, &LocalListOptions::new())
            .expect("tree should list")
            .count()
    );

    let copied = tree.join("copied");
    let _ = filesystem
        .copy_with_options(&source, &copied, &LocalCopyOptions::new())
        .expect("file should be copied");
    let renamed = tree.join("renamed");
    let _ = filesystem
        .rename_with_options(&copied, &renamed, &LocalRenameOptions::new())
        .expect("file should be renamed");
    let _ = filesystem
        .delete_file_with_options(&renamed, &LocalDeleteOptions::new())
        .expect("renamed file should be deleted");

    let temp_file = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("temporary file should be created");
    let temp_directory = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary directory should be created");
    assert!(temp_file.path().exists());
    assert!(temp_directory.path().exists());

    let _ = filesystem
        .delete_file_with_options(&source, &LocalDeleteOptions::new())
        .expect("source file should be deleted");
    let _ = filesystem
        .delete_directory_with_options(&tree, &LocalDeleteOptions::new())
        .expect("tree should be deleted");
}

/// Verifies the Host prefix operation through the configured filesystem.
#[test]
fn test_host_read_prefix() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");

    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    assert_eq!(
        b"pay".as_slice(),
        filesystem
            .read_prefix_with_options(&path, 3, &LocalReadOptions::new())
            .expect("prefix should be readable")
            .as_slice()
    );
}
