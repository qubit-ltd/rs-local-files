// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use crate::LocalFileSystem;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::policy::LocalFileSyncRequirement;

#[test]
fn staged_directory_exposes_bounded_file_synchronization() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("entry"), &mut &b"payload"[..], 16)
        .expect("file should be written");

    assert!(
        stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::Required)
            .expect("required file synchronization should succeed")
    );
    assert!(
        !stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::NotRequired)
            .expect("not-required synchronization should succeed")
    );
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("the staged tree, root, and parent should synchronize");
}

#[cfg(unix)]
#[test]
fn staged_tree_syncs_nested_directories_before_its_root_and_parent() {
    use crate::policy::LocalOwnerAccessPolicy;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("nested/entry"), &mut &b"payload"[..], 16)
        .expect("nested file should be written");

    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("nested files, directories, stage root, and parent should sync");
}

#[cfg(unix)]
#[test]
fn test_review_tree_sync_orders_files_directories_root_and_parent() {
    crate::directory::LocalStagedDirectory::test_review_tree_sync_orders_files_directories_root_and_parent();
}

#[cfg(unix)]
#[test]
fn test_review_tree_sync_stops_after_nested_directory_failure() {
    crate::directory::LocalStagedDirectory::test_review_tree_sync_stops_after_nested_directory_failure();
}
