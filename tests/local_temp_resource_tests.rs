// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;

use qubit_local_files::{
    LocalFileSystem,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
};
use tempfile::tempdir;

/// Verifies temporary-file options bind the parent and apply both affixes.
#[test]
fn test_local_file_system_create_temp_file_applies_options() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut file = LocalFileSystem::create_temp_file(
        &LocalTempFileOptions::new()
            .with_parent(parent.path())
            .with_prefix("upload-")
            .with_suffix(".part"),
    )
    .expect("temporary file should be created");

    assert!(file.path().is_absolute());
    let name = file
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("test affixes are UTF-8");
    assert!(name.starts_with("upload-"));
    assert!(name.ends_with(".part"));
    file.close();
    assert!(file.path().exists());
    file.cleanup().expect("temporary file should be removed");
}

/// Verifies temporary-directory options apply a suffix and cleanup
/// responsibility.
#[test]
fn test_local_file_system_create_temp_directory_applies_suffix() {
    let parent = tempdir().expect("temporary parent should be created");
    let directory = LocalFileSystem::create_temp_directory(
        &LocalTempDirectoryOptions::new()
            .with_parent(parent.path())
            .with_prefix("work-")
            .with_suffix(".tmp"),
    )
    .expect("temporary directory should be created");

    let path = directory.path().to_path_buf();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("test affixes are UTF-8");
    assert!(name.starts_with("work-"));
    assert!(name.ends_with(".tmp"));
    directory
        .cleanup()
        .expect("temporary directory should be removed");
    assert!(!path.exists());
}

/// Verifies invalid affixes fail before leaving a temporary entry.
#[test]
fn test_local_file_system_create_temp_file_rejects_separator_affix() {
    let parent = tempdir().expect("temporary parent should be created");
    let before = fs::read_dir(parent.path())
        .expect("parent should be readable")
        .count();

    let result = LocalFileSystem::create_temp_file(
        &LocalTempFileOptions::new()
            .with_parent(parent.path())
            .with_prefix("unsafe/"),
    );

    assert!(result.is_err());
    assert_eq!(
        before,
        fs::read_dir(parent.path())
            .expect("parent should remain readable")
            .count(),
    );
}
