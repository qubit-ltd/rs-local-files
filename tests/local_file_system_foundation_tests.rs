// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    time::SystemTime,
};

use qubit_local_files::{
    LocalFileKind,
    LocalFileNames,
    LocalFileSystem,
    LocalPathLengthUnit,
};
use tempfile::tempdir;

/// Verifies that host capabilities report the native path-length unit.
#[test]
fn test_local_file_system_capabilities_report_native_path_unit() {
    let capabilities = LocalFileSystem::capabilities();

    #[cfg(unix)]
    assert_eq!(
        Some(LocalPathLengthUnit::Bytes),
        capabilities.path_limit().map(|limit| limit.unit()),
    );
    #[cfg(windows)]
    assert!(capabilities.path_limit().is_none_or(
        |limit| limit.unit() == LocalPathLengthUnit::Utf16CodeUnits
    ),);
}

/// Verifies that generated names are portable single path components.
#[test]
fn test_local_file_names_generate_random_portable_components() {
    let first = LocalFileNames::random_name()
        .expect("a random filename should be generated");
    let second =
        LocalFileNames::random_name_with(Some("prefix-"), Some(".tmp"))
            .expect("a random filename with affixes should be generated");

    assert_ne!(first, second);
    let second_text = second
        .to_str()
        .expect("a portable random filename should be UTF-8");
    assert!(second_text.starts_with("prefix-"));
    assert!(second_text.ends_with(".tmp"));
    LocalFileNames::validate_portable(first.as_os_str())
        .expect("the default random filename should be portable");
    LocalFileNames::validate_portable(second.as_os_str())
        .expect("the affixed random filename should be portable");
}

/// Verifies that metadata observes a final symbolic link without following it.
#[cfg(unix)]
#[test]
fn test_local_file_system_metadata_does_not_follow_final_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    fs::write(&target, b"payload").expect("target should be written");
    symlink(&target, &link).expect("symbolic link should be created");

    let metadata = LocalFileSystem::metadata(&link)
        .expect("symbolic-link metadata should be available");

    assert_eq!(LocalFileKind::Symlink, metadata.kind());
    assert_eq!(
        fs::symlink_metadata(&link)
            .expect("native symlink metadata should be available")
            .len(),
        metadata.len(),
    );
    assert!(metadata.modified_at().is_some());
    let _: Option<SystemTime> = metadata.created_at();
}
