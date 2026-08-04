// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use qubit_local_files::LocalFileKind;
use qubit_local_files::{
    LocalFileNames,
    LocalFileSystem,
    LocalFileSystemCapabilitySupport,
    SizeLimit,
};
#[cfg(unix)]
use std::{
    fs,
    time::SystemTime,
};
#[cfg(unix)]
use tempfile::tempdir;

/// Verifies capabilities do not misrepresent a compile-time path bound as a
/// filesystem-specific limit.
#[test]
fn test_host_file_system_limits_vary_by_path() {
    let limits = LocalFileSystem::host().limits();

    assert_eq!(SizeLimit::VariesByPath, limits.max_path_bytes());
    assert_eq!(SizeLimit::VariesByPath, limits.max_file_name_bytes());
}

/// Verifies space observations are available without caching host limits.
#[test]
fn test_host_file_system_space_observes_existing_directory() {
    let space = LocalFileSystem::host()
        .space_at(std::env::temp_dir().as_path())
        .expect("an existing host directory should be queryable");

    #[cfg(unix)]
    assert!(space.capacity_bytes().is_some());
    #[cfg(not(unix))]
    let _ = &space;
}

/// Verifies capability snapshots distinguish implementation from guarantee.
#[test]
fn test_local_file_system_capabilities_report_support_levels() {
    let capabilities = LocalFileSystem::host().capabilities();

    assert_eq!(
        LocalFileSystemCapabilitySupport::Implemented,
        capabilities.atomic_rename_support(),
    );
    assert_eq!(
        LocalFileSystemCapabilitySupport::Implemented,
        capabilities.atomic_replace_support(),
    );
    assert_eq!(
        LocalFileSystemCapabilitySupport::Implemented,
        capabilities.atomic_temp_persist_support(),
    );
    assert_eq!(
        LocalFileSystemCapabilitySupport::Unknown,
        capabilities.durable_rename_support(),
    );
    assert_eq!(
        LocalFileSystemCapabilitySupport::Unknown,
        capabilities.durable_file_copy_support(),
    );
    assert!(capabilities.rooted_operations_implemented());
    assert!(capabilities.atomic_rename_implemented());
    assert!(capabilities.atomic_replace_implemented());
    assert!(capabilities.atomic_temp_persist_implemented());
    assert!(capabilities.directory_durability_implemented());

    let rooted = tempfile::tempdir().expect("root should be created");
    let rooted_capabilities = LocalFileSystem::rooted(rooted.path())
        .expect("root authority should open")
        .capabilities();
    assert_eq!(capabilities, rooted_capabilities);
}

/// Verifies the host snapshot only advertises native no-replace operations
/// implemented by the host backend on this target.
#[test]
fn test_host_capabilities_match_host_no_replace_backend() {
    let capabilities = LocalFileSystem::host().capabilities();
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    assert!(capabilities.atomic_rename_implemented());
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    assert!(!capabilities.atomic_rename_implemented());
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

    let metadata = LocalFileSystem::host()
        .metadata(&link)
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
