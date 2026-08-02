// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    io::{
        Read,
        Write,
    },
};

use qubit_local_files::{
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalFileKind,
    LocalFileSystem,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
    copy,
    create_directory,
    create_temp_directory,
    create_temp_file,
    delete_directory,
    delete_file,
    list,
    metadata,
    open_reader,
    open_writer,
    rename,
};
use tempfile::tempdir;

/// Verifies the Host convenience surface and configured Host engine use the
/// same native namespace.
#[test]
fn test_host_local_file_system_matches_convenience_metadata() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");

    let convenience = metadata(&path)
        .expect("Host convenience function should inspect the fixture");
    let configured = LocalFileSystem::host()
        .metadata(&path)
        .expect("configured Host filesystem should inspect the fixture");

    assert_eq!(LocalFileKind::File, convenience.kind());
    assert_eq!(convenience.kind(), configured.kind());
    assert_eq!(convenience.len(), configured.len());
}

/// Verifies the complete Host convenience surface delegates to one configured
/// Host filesystem.
#[test]
fn test_host_local_file_system_convenience_workflow() {
    let directory = tempdir().expect("temporary directory should be created");
    let tree = directory.path().join("tree");
    let _ = create_directory(
        &tree,
        &LocalCreateDirectoryOptions::new().with_recursive(),
    )
    .expect("directory should be created");

    let source = tree.join("source");
    let mut writer = open_writer(
        &source,
        &LocalWriteOptions::new(LocalWriteMode::CreateNew),
    )
    .expect("writer should open");
    writer
        .write_all(b"payload")
        .expect("payload should be written");
    let _ = writer.commit().expect("payload should be committed");

    let mut payload = Vec::new();
    open_reader(&source, &LocalReadOptions::new())
        .expect("reader should open")
        .read_to_end(&mut payload)
        .expect("payload should be read");
    assert_eq!(b"payload", payload.as_slice());
    assert_eq!(
        1,
        list(&tree, &LocalListOptions::new())
            .expect("tree should list")
            .count()
    );

    let copied = tree.join("copied");
    let _ = copy(&source, &copied, &LocalCopyOptions::new())
        .expect("file should be copied");
    let renamed = tree.join("renamed");
    let _ = rename(&copied, &renamed, &LocalRenameOptions::new())
        .expect("file should be renamed");
    let _ = delete_file(&renamed, &LocalDeleteOptions::new())
        .expect("renamed file should be deleted");

    let temp_file = create_temp_file(&LocalTempFileOptions::new())
        .expect("temporary file should be created");
    let temp_directory =
        create_temp_directory(&LocalTempDirectoryOptions::new())
            .expect("temporary directory should be created");
    assert!(temp_file.path().exists());
    assert!(temp_directory.path().exists());

    let _ = delete_file(&source, &LocalDeleteOptions::new())
        .expect("source file should be deleted");
    let _ = delete_directory(&tree, &LocalDeleteOptions::new())
        .expect("tree should be deleted");
}
