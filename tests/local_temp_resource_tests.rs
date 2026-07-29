// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use qubit_local_files::{
    LocalFileSystem,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::ffi::OsStringExt;
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
    let mut directory = LocalFileSystem::create_temp_directory(
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

/// Verifies temporary-resource creation creates a missing parent hierarchy
/// before reserving either entry kind.
#[test]
fn test_local_file_system_create_temp_resources_create_missing_parent() {
    let workspace = tempdir().expect("temporary workspace should be created");
    let file_parent = workspace.path().join("file-parent/nested");
    let directory_parent = workspace.path().join("directory-parent/nested");

    let mut file = LocalFileSystem::create_temp_file(
        &LocalTempFileOptions::new().with_parent(&file_parent),
    )
    .expect("temporary file should create its missing parent");
    let file_path = file.path().to_path_buf();
    assert!(file_parent.is_dir());
    assert!(file_path.is_file());
    file.cleanup().expect("temporary file should be removed");

    let mut directory = LocalFileSystem::create_temp_directory(
        &LocalTempDirectoryOptions::new().with_parent(&directory_parent),
    )
    .expect("temporary directory should create its missing parent");
    let directory_path = directory.path().to_path_buf();
    assert!(directory_parent.is_dir());
    assert!(directory_path.is_dir());
    directory
        .cleanup()
        .expect("temporary directory should be removed");
}

/// Verifies host no-replace persistence rejects an interior NUL in the target
/// name without publishing the temporary file.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn test_local_temp_file_persist_rejects_interior_nul_target() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::create_temp_file(
        &LocalTempFileOptions::new().with_parent(parent.path()),
    )
    .expect("temporary file should be created");
    let source = temporary.path().to_path_buf();
    let target = parent
        .path()
        .join(std::ffi::OsString::from_vec(b"target\0name".to_vec()));

    let error = temporary
        .persist(&target)
        .expect_err("interior NUL must be rejected by native no-replace move");
    assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
    let (_io, temporary, _requested, _resolved, _stage) = error.into_parts();
    drop(temporary);

    assert!(source.exists());
    fs::remove_file(source)
        .expect("indeterminate temporary file should be removed manually");
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
