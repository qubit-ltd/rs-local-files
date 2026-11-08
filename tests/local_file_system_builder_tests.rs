// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public construction and instance-configuration coverage.

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalRenameOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::path::LocalFileSystemScope;
use qubit_local_files::policy::LocalSymlinkPolicy;

#[test]
fn test_rooted_default_options_accessors_and_setters_round_trip() {
    let fixture = tempfile::tempdir().expect("fixture");
    let mut filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    let _ = filesystem.default_read_options();
    filesystem
        .set_default_read_options(LocalReadOptions::new())
        .expect("read defaults");
    let _ = filesystem.default_write_options();
    filesystem
        .set_default_write_options(LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("write defaults");
    let _ = filesystem.default_list_options();
    filesystem
        .set_default_list_options(LocalListOptions::new())
        .expect("list defaults");
    let _ = filesystem.default_copy_options();
    filesystem
        .set_default_copy_options(LocalCopyOptions::new())
        .expect("copy defaults");
    let _ = filesystem.default_create_directory_options();
    filesystem
        .set_default_create_directory_options(LocalCreateDirectoryOptions::new())
        .expect("create-directory defaults");
    let _ = filesystem.default_delete_options();
    filesystem
        .set_default_delete_options(LocalDeleteOptions::new())
        .expect("delete defaults");
    let _ = filesystem.default_rename_options();
    filesystem
        .set_default_rename_options(LocalRenameOptions::new())
        .expect("rename defaults");
    let _ = filesystem.default_temp_file_options();
    filesystem
        .set_default_temp_file_options(LocalTempFileOptions::new())
        .expect("temp-file defaults");
    let _ = filesystem.default_temp_directory_options();
    filesystem
        .set_default_temp_directory_options(LocalTempDirectoryOptions::new())
        .expect("temp-directory defaults");
}

#[test]
fn test_rooted_symlink_policy_rejection_preserves_the_previous_default() {
    let fixture = tempfile::tempdir().expect("fixture");
    let mut filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let original = filesystem.symlink_policy();

    let error = filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect_err("rooted filesystems cannot follow links across scope");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(filesystem.symlink_policy(), original);
}

#[test]
fn test_cloned_filesystem_shares_authority_but_copies_configuration() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut filesystem = LocalFileSystem::rooted(root.path()).expect("rooted filesystem");
    filesystem
        .set_default_list_options(
            LocalListOptions::new()
                .with_max_entries(32)
                .with_max_open_directories(4),
        )
        .expect("list defaults should be accepted");

    let mut cloned = filesystem.clone();
    assert_eq!(cloned.scope(), LocalFileSystemScope::Rooted);
    assert_eq!(cloned.default_list_options().max_entries(), Some(32));
    cloned
        .set_default_list_options(LocalListOptions::new())
        .expect("clone defaults should remain configurable");
    assert_eq!(filesystem.default_list_options().max_entries(), Some(32));
    assert_eq!(cloned.default_list_options().max_entries(), None);
}

#[test]
fn test_configuration_setters_reject_invalid_options_transactionally() {
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let original = *filesystem.default_list_options();
    let error = filesystem
        .set_default_list_options(LocalListOptions::new().with_max_open_directories(0))
        .expect_err("zero open-directory budgets must be rejected");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(filesystem.default_list_options(), &original);
}
