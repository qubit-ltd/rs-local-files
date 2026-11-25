// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(all(windows, feature = "test-support"))]

use std::ffi::OsStr;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreePublishOptions;
use qubit_local_files::policy::LocalFileSyncRequirement;
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalTreeFinalAccess;
use qubit_local_files::test_support::install_test_fault;

fn parent(path: &Path) -> LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(path, &LocalDirectoryOpenOptions::new())
        .expect("parent authority")
}

fn stage_with_file(parent: &LocalDirectoryHandle) -> qubit_local_files::directory::LocalStagedDirectory {
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    stage
}

#[test]
fn test_preferred_sync_open_failure_preserves_access_and_reports_unsynced() {
    let scratch = tempfile::tempdir().expect("fixture");
    let parent = parent(scratch.path());
    let stage = stage_with_file(&parent);
    let _fault = install_test_fault("windows-sync-final-open-permission-denied").expect("sync-open selector");

    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_file_sync(LocalFileSyncRequirement::Preferred),
        )
        .expect("Preferred sync capability may degrade");
    assert!(!outcome.file_synced());
}

#[test]
fn test_required_sync_open_failure_preserves_stage_before_access_mutation() {
    let scratch = tempfile::tempdir().expect("fixture");
    let parent = parent(scratch.path());
    let stage = stage_with_file(&parent);
    let _fault = install_test_fault("windows-sync-final-open-permission-denied").expect("sync-open selector");

    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_file_sync(LocalFileSyncRequirement::Required),
        )
        .expect_err("Required sync capability must be available");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeMutationState::NotStarted,
        error.mutation_state()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Preserved,
        error.access_state()
    );
    error.into_stage().expect("stage retained").cleanup().expect("cleanup");
}

#[test]
fn test_preferred_sync_propagates_non_permission_open_failure() {
    let scratch = tempfile::tempdir().expect("fixture");
    let parent = parent(scratch.path());
    let stage = stage_with_file(&parent);
    let _fault = install_test_fault("tree-sync-main-open-failure").expect("open failure selector");

    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_file_sync(LocalFileSyncRequirement::Preferred),
        )
        .expect_err("a non-permission open failure must not degrade Preferred");
    assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.cause().kind());
    error.into_stage().expect("stage retained").cleanup().expect("cleanup");
}

#[test]
fn test_preferred_access_sync_open_failure_degrades_current_and_read_only_policies() {
    for access in [
        LocalTreeFinalAccess::CurrentUserOnly,
        LocalTreeFinalAccess::ReadOnlyCurrentUser,
    ] {
        let scratch = tempfile::tempdir().expect("fixture");
        let parent = parent(scratch.path());
        let stage = stage_with_file(&parent);
        let _fault = install_test_fault("tree-access-sync-capability").expect("access sync selector");
        let outcome = stage
            .publish_new(
                &parent,
                OsStr::new("published"),
                &LocalTreePublishOptions::new()
                    .with_access(access)
                    .with_file_sync(LocalFileSyncRequirement::Preferred),
            )
            .expect("Preferred sync capability may degrade after identity check");
        assert!(!outcome.file_synced(), "{access:?} must report the sync downgrade");
    }
}

#[test]
fn test_preferred_access_propagates_non_permission_sync_open_failure() {
    let scratch = tempfile::tempdir().expect("fixture");
    let parent = parent(scratch.path());
    let stage = stage_with_file(&parent);
    let _fault = install_test_fault("tree-access-main-open-failure").expect("access failure selector");

    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_access(LocalTreeFinalAccess::CurrentUserOnly)
                .with_file_sync(LocalFileSyncRequirement::Preferred),
        )
        .expect_err("a non-permission access-open failure must propagate");
    assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.cause().kind());
    error.into_stage().expect("stage retained").cleanup().expect("cleanup");
}

#[test]
fn test_preferred_owner_policy_sync_open_failure_publishes_unsynced() {
    let scratch = tempfile::tempdir().expect("fixture");
    let parent = parent(scratch.path());
    let stage = stage_with_file(&parent);
    let _fault = install_test_fault("owner-policy-sync-capability").expect("owner policy sync selector");
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_01ef, 0x0016_019f);

    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Preferred),
        )
        .expect("Preferred owner policy sync capability may degrade");
    assert!(!outcome.file_synced());
    assert_eq!(Some(&policy), outcome.owner_access_policy());
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::OwnerPolicyApplied,
        outcome.access_state()
    );
}

#[test]
fn test_owner_policy_replaced_stage_name_blocks_root_acl_mutation() {
    let scratch = tempfile::tempdir().expect("fixture");
    let parent = parent(scratch.path());
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let expected_identity = stage.identity().cloned().expect("stage identity");
    let native = scratch.path().join(stage.relative_name());
    let mut backup_name = stage.relative_name().to_os_string();
    backup_name.push(".owner-policy-original");
    let backup_relative = std::path::PathBuf::from(backup_name);
    let backup = scratch.path().join(&backup_relative);
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_01ef, 0x0016_019f);
    let _fault = install_test_fault("owner-policy-replace-stage-before-root").expect("replacement selector");

    let error = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())
        .expect_err("replaced stage name must block root policy mutation");
    assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.cause().kind());
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Mixed,
        error.access_state()
    );
    let replacement = parent
        .observe_entry(Path::new(stage.relative_name()))
        .expect("replacement stage name remains observable");
    assert_ne!(Some(&expected_identity), replacement.identity());
    let retained_stage = parent
        .observe_entry(&backup_relative)
        .expect("retained stage remains observable at the backup name");
    assert_eq!(Some(&expected_identity), retained_stage.identity());
    assert_eq!(
        &b"payload"[..],
        std::fs::read(backup.join("payload"))
            .expect("retained payload remains readable")
            .as_slice()
    );
    assert!(
        std::fs::read_dir(&native)
            .expect("replacement directory remains present")
            .next()
            .is_none(),
        "root policy must not mutate or populate the replacement directory"
    );

    drop(error);
    std::fs::remove_dir(&native).expect("remove injected replacement");
    std::fs::rename(&backup, &native).expect("restore original stage binding");
    stage.cleanup().expect("cleanup restored original stage");
}

#[test]
fn test_preferred_owner_policy_propagates_non_permission_open_failure() {
    let scratch = tempfile::tempdir().expect("fixture");
    let parent = parent(scratch.path());
    let stage = stage_with_file(&parent);
    let _fault = install_test_fault("owner-policy-main-open-failure").expect("owner policy failure selector");
    let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_01ef, 0x0016_019f);

    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Preferred),
        )
        .expect_err("a non-permission owner-policy open failure must propagate");
    assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.cause().kind());
    error.into_stage().expect("stage retained").cleanup().expect("cleanup");
}
