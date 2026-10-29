// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreePublishOptions;
use qubit_local_files::outcome::LocalTreePublication;
use qubit_local_files::policy::LocalFileSyncRequirement;
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use tempfile::tempdir;

fn private_stage(parent: &LocalDirectoryHandle) -> qubit_local_files::directory::LocalStagedDirectory {
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
        .expect("private stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"readonly payload"[..], 64)
        .expect("write payload");
    stage
}

fn writable_stage(parent: &LocalDirectoryHandle) -> qubit_local_files::directory::LocalStagedDirectory {
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("writable stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"readonly payload"[..], 64)
        .expect("write payload");
    stage
}

fn parent_handle(path: &Path) -> LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host")
        .open_directory_with_options(path, &LocalDirectoryOpenOptions::new())
        .expect("parent handle")
}

fn assert_readonly_payload(path: &Path) {
    let metadata = std::fs::metadata(path).expect("payload metadata");
    assert_eq!(0o400, metadata.permissions().mode() & 0o777);
    assert_eq!(b"readonly payload"[..], std::fs::read(path).expect("payload bytes"));
}

fn assert_private_stage(path: &Path) {
    let metadata = std::fs::metadata(path).expect("stage metadata");
    assert_eq!(0o700, metadata.permissions().mode() & 0o777);
    assert_readonly_payload(&path.join("payload"));
}

#[test]
fn sync_files_required_reads_readonly_staged_file() {
    let scratch = tempdir().expect("scratch");
    let parent = parent_handle(scratch.path());
    let stage = private_stage(&parent);
    let stage_path = scratch.path().join(stage.relative_name());

    assert_private_stage(&stage_path);
    assert!(
        stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::Required)
            .expect("sync readonly file")
    );
    assert_private_stage(&stage_path);
}

#[test]
fn sync_tree_synchronizes_readonly_staged_file() {
    let scratch = tempdir().expect("scratch");
    let parent = parent_handle(scratch.path());
    let stage = private_stage(&parent);
    let stage_path = scratch.path().join(stage.relative_name());

    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("sync readonly tree");
    assert_private_stage(&stage_path);
}

#[test]
fn publish_new_required_syncs_readonly_staged_file() {
    let scratch = tempdir().expect("scratch");
    let parent = parent_handle(scratch.path());
    let stage = writable_stage(&parent);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);

    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Required),
        )
        .expect("publish readonly tree after required sync");

    assert!(outcome.file_synced());
    assert_eq!(LocalTreePublication::Published, outcome.publication());
    assert_private_stage(&scratch.path().join("published"));
}
