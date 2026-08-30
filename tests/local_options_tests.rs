// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public option-value coverage for local filesystem operations.

use std::hint::black_box;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalAtomicityRequirement;
use qubit_local_files::LocalCopyConflictPolicy;
use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalCopySourceMode;
use qubit_local_files::LocalCopyTypeConflictPolicy;
use qubit_local_files::LocalCreateDirectoryOptions;
use qubit_local_files::LocalDeleteOptions;
use qubit_local_files::LocalDirectoryReopenPolicy;
use qubit_local_files::LocalDurabilityRequirement;
use qubit_local_files::LocalListOptions;
use qubit_local_files::LocalMetadataPreservePolicy;
use qubit_local_files::LocalPersistOptions;
use qubit_local_files::LocalReadOptions;
use qubit_local_files::LocalRenameOptions;
use qubit_local_files::LocalSymlinkPolicy;
use qubit_local_files::LocalTempDirectoryOptions;
use qubit_local_files::LocalTempFileOptions;
use qubit_local_files::LocalWalkErrorPolicy;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;

/// Verifies directory and deletion builders retain every configured policy.
#[test]
fn test_directory_and_delete_option_builders_retain_policies() {
    let create = black_box(LocalCreateDirectoryOptions::new as fn() -> LocalCreateDirectoryOptions);
    let create_recursive = black_box(
        LocalCreateDirectoryOptions::with_recursive as fn(LocalCreateDirectoryOptions) -> LocalCreateDirectoryOptions,
    );
    let create_exists_ok = black_box(
        LocalCreateDirectoryOptions::with_exists_ok as fn(LocalCreateDirectoryOptions) -> LocalCreateDirectoryOptions,
    );
    let directory = create_exists_ok(create_recursive(create()));
    assert!(black_box(LocalCreateDirectoryOptions::recursive)(&directory));
    assert!(black_box(LocalCreateDirectoryOptions::exists_ok)(&directory));

    let delete = black_box(LocalDeleteOptions::new as fn() -> LocalDeleteOptions);
    let delete_recursive =
        black_box(LocalDeleteOptions::with_recursive as fn(LocalDeleteOptions) -> LocalDeleteOptions);
    let delete_missing_ok =
        black_box(LocalDeleteOptions::with_missing_ok as fn(LocalDeleteOptions) -> LocalDeleteOptions);
    let deletion = delete_missing_ok(delete_recursive(delete()));
    assert!(black_box(LocalDeleteOptions::recursive)(&deletion));
    assert!(black_box(LocalDeleteOptions::missing_ok)(&deletion));
}

/// Verifies listing and reader builders retain their traversal and retry data.
#[test]
fn test_list_and_read_option_builders_retain_policies() {
    let list = black_box(LocalListOptions::new as fn() -> LocalListOptions);
    let list_recursive = black_box(LocalListOptions::with_recursive as fn(LocalListOptions) -> LocalListOptions);
    let list_policy = black_box(
        LocalListOptions::with_symlink_policy as fn(LocalListOptions, LocalSymlinkPolicy) -> LocalListOptions,
    );
    let list_max_depth = black_box(LocalListOptions::with_max_depth as fn(LocalListOptions, usize) -> LocalListOptions);
    let list_max_handles =
        black_box(LocalListOptions::with_max_open_directories as fn(LocalListOptions, usize) -> LocalListOptions);
    let listing = list_max_handles(
        list_max_depth(
            list_policy(list_recursive(list()), LocalSymlinkPolicy::FollowWithinScope),
            3,
        ),
        7,
    );
    let listing = listing
        .with_max_entries(11)
        .with_max_seen_name_bytes(128)
        .with_deadline(Duration::from_secs(2));
    assert!(black_box(LocalListOptions::recursive)(&listing));
    assert_eq!(
        Some(LocalSymlinkPolicy::FollowWithinScope),
        black_box(LocalListOptions::symlink_policy)(&listing),
    );
    assert_eq!(black_box(LocalListOptions::max_depth)(&listing), Some(3));
    assert_eq!(black_box(LocalListOptions::max_open_directories)(&listing), Some(7));
    assert_eq!(listing.max_entries(), Some(11));
    assert_eq!(listing.max_seen_name_bytes(), Some(128));
    assert_eq!(listing.deadline(), Some(Duration::from_secs(2)));
    assert_eq!(
        LocalWalkErrorPolicy::FailFast,
        black_box(LocalListOptions::error_policy)(&listing),
    );
    let listing = black_box(
        LocalListOptions::with_reopen_policy as fn(LocalListOptions, LocalDirectoryReopenPolicy) -> LocalListOptions,
    )(listing, LocalDirectoryReopenPolicy::Fail);
    let listing = black_box(
        LocalListOptions::with_error_policy as fn(LocalListOptions, LocalWalkErrorPolicy) -> LocalListOptions,
    )(listing, LocalWalkErrorPolicy::Continue);
    assert_eq!(
        LocalDirectoryReopenPolicy::Fail,
        black_box(LocalListOptions::reopen_policy)(&listing),
    );
    assert_eq!(
        LocalWalkErrorPolicy::Continue,
        black_box(LocalListOptions::error_policy)(&listing),
    );

    let timeout = Duration::from_millis(25);
    let reader = black_box(LocalReadOptions::new as fn() -> LocalReadOptions)();
    let reader = black_box(
        LocalReadOptions::with_open_retry_timeout as fn(LocalReadOptions, Duration) -> LocalReadOptions,
    )(reader, timeout);
    assert_eq!(black_box(LocalReadOptions::open_retry_timeout)(&reader), Some(timeout));
}

/// Verifies copy, rename, and write builders preserve all publication rules.
#[test]
fn test_copy_rename_and_write_option_builders_retain_policies() {
    let copy = black_box(LocalCopyOptions::new())
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions)
        .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .with_file_source()
        .with_tree_source()
        .with_parent()
        .with_atomicity(LocalAtomicityRequirement::Required)
        .with_durability(LocalDurabilityRequirement::Required)
        .with_max_depth(3)
        .with_max_entries(11)
        .with_max_bytes(1024)
        .with_max_open_directories(4)
        .with_deadline(Duration::from_secs(2));
    assert_eq!(copy.conflict(), LocalCopyConflictPolicy::Overwrite);
    assert_eq!(copy.type_conflict(), LocalCopyTypeConflictPolicy::Replace);
    assert_eq!(copy.preserve_metadata(), LocalMetadataPreservePolicy::Permissions);
    assert_eq!(
        copy.symlink_policy_override(),
        Some(LocalSymlinkPolicy::FollowWithinScope)
    );
    assert_eq!(copy.source_mode(), LocalCopySourceMode::Tree);
    assert!(copy.creates_parent());
    assert_eq!(copy.atomicity(), LocalAtomicityRequirement::Required);
    assert_eq!(copy.durability(), LocalDurabilityRequirement::Required);
    assert_eq!(copy.max_depth(), Some(3));
    assert_eq!(copy.max_entries(), Some(11));
    assert_eq!(copy.max_bytes(), Some(1024));
    assert_eq!(copy.max_open_directories(), Some(4));
    assert_eq!(copy.deadline(), Some(Duration::from_secs(2)));

    let rename = black_box(LocalRenameOptions::new())
        .with_overwrite()
        .with_durability(LocalDurabilityRequirement::Preferred);
    assert!(rename.overwrite());
    assert_eq!(rename.durability(), LocalDurabilityRequirement::Preferred);

    let timeout = Duration::from_secs(1);
    let writer = black_box(LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .with_parent()
        .with_atomicity(LocalAtomicityRequirement::Required)
        .with_durability(LocalDurabilityRequirement::Preferred)
        .with_open_retry_timeout(timeout);
    assert_eq!(writer.mode(), LocalWriteMode::CreateNew);
    assert!(writer.creates_parent());
    assert_eq!(writer.atomicity(), LocalAtomicityRequirement::Required);
    assert_eq!(writer.durability(), LocalDurabilityRequirement::Preferred);
    assert_eq!(writer.open_retry_timeout(), Some(timeout));
}

/// Verifies temporary-resource builders retain path and naming configuration.
#[test]
fn test_temporary_resource_option_builders_retain_configuration() {
    let parent = Path::new("temporary-parent");
    let with_suffix =
        black_box(LocalTempFileOptions::with_suffix as fn(LocalTempFileOptions, &str) -> LocalTempFileOptions);
    let file = with_suffix(
        black_box(LocalTempFileOptions::new())
            .with_parent(parent)
            .with_prefix("file-"),
        ".tmp",
    )
    .with_max_attempts(4)
    .with_create_parent();
    assert_eq!(file.parent(), Some(parent));
    assert_eq!(file.prefix(), Some("file-"));
    assert_eq!(file.suffix(), Some(".tmp"));
    assert_eq!(file.max_attempts(), Some(4));
    assert!(file.creates_parent());

    let with_directory_max_attempts = black_box(
        LocalTempDirectoryOptions::with_max_attempts
            as fn(LocalTempDirectoryOptions, usize) -> LocalTempDirectoryOptions,
    );
    let directory = with_directory_max_attempts(
        black_box(LocalTempDirectoryOptions::new())
            .with_parent(parent)
            .with_prefix("directory-")
            .with_suffix(".tmp"),
        5,
    );
    assert_eq!(directory.parent(), Some(parent));
    assert_eq!(directory.prefix(), Some("directory-"));
    assert_eq!(directory.suffix(), Some(".tmp"));
    assert_eq!(directory.max_attempts(), Some(5));
    assert!(!directory.creates_parent());
    assert!(directory.with_create_parent().creates_parent());
}

/// Verifies every option type exposes the documented conservative default.
#[test]
fn test_option_defaults_match_their_constructors() {
    let create_default = black_box(LocalCreateDirectoryOptions::default as fn() -> LocalCreateDirectoryOptions);
    let delete_default = black_box(LocalDeleteOptions::default as fn() -> LocalDeleteOptions);
    let list_default = black_box(LocalListOptions::default as fn() -> LocalListOptions);
    let read_default = black_box(LocalReadOptions::default as fn() -> LocalReadOptions);
    let copy_default = black_box(LocalCopyOptions::default as fn() -> LocalCopyOptions);
    let rename_default = black_box(LocalRenameOptions::default as fn() -> LocalRenameOptions);
    let temp_file_default = black_box(LocalTempFileOptions::default as fn() -> LocalTempFileOptions);
    let temp_directory_default = black_box(LocalTempDirectoryOptions::default as fn() -> LocalTempDirectoryOptions);
    assert_eq!(create_default(), LocalCreateDirectoryOptions::new());
    assert_eq!(delete_default(), LocalDeleteOptions::new());
    assert_eq!(list_default(), LocalListOptions::new());
    assert_eq!(read_default(), LocalReadOptions::new());
    assert_eq!(copy_default(), LocalCopyOptions::new());
    assert_eq!(rename_default(), LocalRenameOptions::new());
    assert_eq!(temp_file_default(), LocalTempFileOptions::new());
    assert_eq!(temp_directory_default(), LocalTempDirectoryOptions::new());
}

/// Verifies conservative option values are observable before any builder is
/// applied.
#[test]
fn test_option_constructors_expose_conservative_values() {
    let create = black_box(LocalCreateDirectoryOptions::new as fn() -> _)();
    assert!(!black_box(LocalCreateDirectoryOptions::recursive)(&create));
    assert!(!black_box(LocalCreateDirectoryOptions::exists_ok)(&create));

    let deletion = black_box(LocalDeleteOptions::new as fn() -> _)();
    assert!(!black_box(LocalDeleteOptions::recursive)(&deletion));
    assert!(!black_box(LocalDeleteOptions::missing_ok)(&deletion));

    let listing = black_box(LocalListOptions::new as fn() -> _)();
    assert!(!black_box(LocalListOptions::recursive)(&listing));
    assert_eq!(None, black_box(LocalListOptions::symlink_policy)(&listing));
    assert_eq!(None, black_box(LocalListOptions::max_depth)(&listing));

    let copy = black_box(LocalCopyOptions::new as fn() -> _)();
    assert_eq!(
        LocalCopyConflictPolicy::Fail,
        black_box(LocalCopyOptions::conflict)(&copy)
    );
    assert_eq!(
        LocalCopyTypeConflictPolicy::Fail,
        black_box(LocalCopyOptions::type_conflict)(&copy)
    );
    assert_eq!(
        LocalMetadataPreservePolicy::None,
        black_box(LocalCopyOptions::preserve_metadata)(&copy)
    );
    assert_eq!(None, black_box(LocalCopyOptions::symlink_policy_override)(&copy));
    assert_eq!(
        LocalCopySourceMode::Auto,
        black_box(LocalCopyOptions::source_mode)(&copy)
    );
    assert_eq!(
        LocalAtomicityRequirement::Preferred,
        black_box(LocalCopyOptions::atomicity)(&copy)
    );
    assert_eq!(
        LocalDurabilityRequirement::NotRequired,
        black_box(LocalCopyOptions::durability)(&copy)
    );

    let rename = black_box(LocalRenameOptions::new as fn() -> _)();
    assert!(!black_box(LocalRenameOptions::overwrite)(&rename));
    assert_eq!(
        LocalDurabilityRequirement::NotRequired,
        black_box(LocalRenameOptions::durability)(&rename)
    );

    let file = black_box(LocalTempFileOptions::default as fn() -> _)();
    assert_eq!(None, black_box(LocalTempFileOptions::parent)(&file));
    assert_eq!(None, black_box(LocalTempFileOptions::prefix)(&file));
    assert_eq!(None, black_box(LocalTempFileOptions::suffix)(&file));
    assert_eq!(None, black_box(LocalTempFileOptions::max_attempts)(&file));

    let directory = black_box(LocalTempDirectoryOptions::default as fn() -> _)();
    assert_eq!(None, black_box(LocalTempDirectoryOptions::parent)(&directory));
    assert_eq!(None, black_box(LocalTempDirectoryOptions::prefix)(&directory));
    assert_eq!(None, black_box(LocalTempDirectoryOptions::suffix)(&directory));
    assert_eq!(None, black_box(LocalTempDirectoryOptions::max_attempts)(&directory));

    let writer = black_box(LocalWriteOptions::new as fn(_) -> _)(LocalWriteMode::CreateOrReplace);
    assert_eq!(
        LocalWriteMode::CreateOrReplace,
        black_box(LocalWriteOptions::mode)(&writer)
    );
    assert!(!black_box(LocalWriteOptions::creates_parent)(&writer));
    assert_eq!(
        LocalAtomicityRequirement::Preferred,
        black_box(LocalWriteOptions::atomicity)(&writer)
    );
    assert_eq!(
        LocalDurabilityRequirement::NotRequired,
        black_box(LocalWriteOptions::durability)(&writer)
    );
    assert_eq!(None, black_box(LocalWriteOptions::open_retry_timeout)(&writer));
}

/// Verifies caller-owned optional budgets are absent by default and can be
/// explicitly cleared after cloning configured options.
#[test]
fn test_optional_budgets_can_be_cleared() {
    let listing = LocalListOptions::new()
        .with_max_depth(1)
        .with_max_entries(2)
        .with_max_seen_name_bytes(3)
        .with_max_open_directories(4)
        .with_deadline(Duration::from_secs(5))
        .without_max_depth()
        .without_max_entries()
        .without_max_seen_name_bytes()
        .without_max_open_directories()
        .without_deadline();
    assert_eq!(listing.max_depth(), None);
    assert_eq!(listing.max_entries(), None);
    assert_eq!(listing.max_seen_name_bytes(), None);
    assert_eq!(listing.max_open_directories(), None);
    assert_eq!(listing.deadline(), None);

    let copy = LocalCopyOptions::new()
        .with_max_depth(1)
        .with_max_entries(2)
        .with_max_bytes(3)
        .with_max_open_directories(4)
        .with_deadline(Duration::from_secs(5))
        .without_max_depth()
        .without_max_entries()
        .without_max_bytes()
        .without_max_open_directories()
        .without_deadline();
    assert_eq!(copy.max_depth(), None);
    assert_eq!(copy.max_entries(), None);
    assert_eq!(copy.max_bytes(), None);
    assert_eq!(copy.max_open_directories(), None);
    assert_eq!(copy.deadline(), None);

    let read = LocalReadOptions::new()
        .with_open_retry_timeout(Duration::ZERO)
        .without_open_retry_timeout();
    assert_eq!(read.open_retry_timeout(), None);
    let write = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_open_retry_timeout(Duration::ZERO)
        .without_open_retry_timeout();
    assert_eq!(write.open_retry_timeout(), None);

    let file = LocalTempFileOptions::new().with_max_attempts(1).without_max_attempts();
    assert_eq!(file.max_attempts(), None);
    let directory = LocalTempDirectoryOptions::new()
        .with_max_attempts(1)
        .without_max_attempts();
    assert_eq!(directory.max_attempts(), None);
}

/// Verifies temporary-resource persistence defaults to no replacement and can
/// explicitly opt into replacement.
#[test]
fn test_persist_options_expose_overwrite_policy() {
    let conservative = black_box(LocalPersistOptions::default as fn() -> _)();
    assert!(!black_box(LocalPersistOptions::overwrites)(&conservative));

    let replacing = black_box(LocalPersistOptions::with_overwrite as fn(_) -> _)(black_box(
        LocalPersistOptions::new as fn() -> _,
    )());
    assert!(black_box(LocalPersistOptions::overwrites)(&replacing));
    let with_parent = black_box(LocalPersistOptions::with_create_parent as fn(_) -> _)(LocalPersistOptions::new());
    assert!(black_box(LocalPersistOptions::creates_parent)(&with_parent));
    assert_eq!(LocalPersistOptions::new(), LocalPersistOptions::default());
}
