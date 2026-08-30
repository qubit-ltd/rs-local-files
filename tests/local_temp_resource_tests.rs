// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::ffi::OsStringExt;

use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileSystem;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use qubit_local_files::LocalPersistFailureState;
use qubit_local_files::LocalTempDirectoryOptions;
use qubit_local_files::LocalTempFileOptions;
use tempfile::tempdir;

/// Verifies temporary-file options bind the parent and apply both affixes.
#[test]
fn test_local_file_system_create_temp_file_applies_options() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(
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
    let mut directory = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(
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
    directory.cleanup().expect("temporary directory should be removed");
    assert!(!path.exists());
}

/// Verifies explicit temporary-resource options create a missing parent
/// hierarchy before reserving either entry kind.
#[test]
fn test_local_file_system_create_temp_resources_create_missing_parent() {
    let workspace = tempdir().expect("temporary workspace should be created");
    let file_parent = workspace.path().join("file-parent/nested");
    let directory_parent = workspace.path().join("directory-parent/nested");

    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(&file_parent)
                .with_create_parent(),
        )
        .expect("temporary file should create its missing parent");
    let file_path = file.path().to_path_buf();
    assert!(file_parent.is_dir());
    assert!(file_path.is_file());
    file.cleanup().expect("temporary file should be removed");

    let mut directory = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_parent(&directory_parent)
                .with_create_parent(),
        )
        .expect("temporary directory should create its missing parent");
    let directory_path = directory.path().to_path_buf();
    assert!(directory_parent.is_dir());
    assert!(directory_path.is_dir());
    directory.cleanup().expect("temporary directory should be removed");
}

/// Verifies parent creation reports a stable type error when the requested
/// parent already exists as a file.
#[test]
fn test_local_file_system_create_temp_file_rejects_file_parent() {
    let directory = tempdir().expect("temporary directory should be created");
    let parent = directory.path().join("not-a-directory");
    std::fs::write(&parent, b"file").expect("file parent fixture should be written");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");

    let error = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent).with_create_parent())
        .expect_err("a file cannot be used as a temporary parent");

    assert_eq!(LocalFileErrorKind::NotDirectory, error.kind());
}

/// Verifies host no-replace persistence rejects an interior NUL in the target
/// name without publishing the temporary file.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn test_local_temp_file_persist_rejects_interior_nul_target() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let source = temporary.path().to_path_buf();
    let target = parent
        .path()
        .join(std::ffi::OsString::from_vec(b"target\0name".to_vec()));

    let error = temporary
        .persist(&target)
        .expect_err("interior NUL must be rejected by native no-replace move");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalPersistFailureState::NotPublished, error.state());
    let (_io, mut temporary, _requested, _resolved, _stage) = error.into_parts();
    temporary
        .cleanup()
        .expect("unpublished temporary file should retain cleanup authority");

    assert!(!source.exists());
}

/// Verifies invalid affixes fail before leaving a temporary entry.
#[test]
fn test_local_file_system_create_temp_file_rejects_separator_affix() {
    let parent = tempdir().expect("temporary parent should be created");
    let before = fs::read_dir(parent.path()).expect("parent should be readable").count();

    let result = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(
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
