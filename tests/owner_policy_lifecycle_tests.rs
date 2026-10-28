// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Owner access policies must be applied and retained throughout stage use.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalTreeValidationOptions;
use qubit_local_files::observation::LocalObservationRequirement;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalPrivacyRequirement;
use tempfile::TempDir;

fn parent(scratch: &TempDir) -> qubit_local_files::directory::LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority")
}

#[test]
fn test_custom_owner_policies_create_exact_directory_and_file_modes() {
    for (directory_mode, file_mode) in [(0o700, 0o600), (0o700, 0o400)] {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let parent = parent(&scratch);
        let policy = LocalOwnerAccessPolicy::new().with_unix_modes(directory_mode, file_mode);
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
            .expect("stage with exact owner policy");
        stage
            .write_file_new(Path::new("nested/payload"), &mut &b"payload"[..], 64)
            .expect("create nested file with policy");

        let stage_path = scratch.path().join(stage.relative_name());
        assert_eq!(
            directory_mode,
            fs::metadata(&stage_path).expect("stage metadata").mode() & 0o777
        );
        assert_eq!(
            directory_mode,
            fs::metadata(stage_path.join("nested")).expect("nested metadata").mode() & 0o777
        );
        assert_eq!(
            file_mode,
            fs::metadata(stage_path.join("nested/payload"))
                .expect("file metadata")
                .mode()
                & 0o777
        );
        let observed = stage
            .directory()
            .security_observation(
                &LocalSecurityObservationOptions::new().with_access(LocalObservationRequirement::Required),
            )
            .expect("same-object access observation");
        assert_eq!(
            Some(directory_mode),
            observed
                .access()
                .and_then(|access| access.unix())
                .map(|access| access.mode())
        );
    }
}

#[test]
fn test_custom_read_only_policy_reopens_with_required_privacy() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400);
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
        .expect("stage with read-only policy");
    let name = stage.relative_name().to_os_string();
    let identity = stage.identity().expect("stage identity").clone();
    drop(stage.retain_in_place());

    let reopened = parent.open_staged_directory(
        Path::new(&name),
        &identity,
        &LocalTreeValidationOptions::new()
            .with_owner_access_policy(policy)
            .with_privacy(LocalPrivacyRequirement::Required),
    );
    assert!(
        reopened.is_ok(),
        "custom read-only owner policy and required privacy must share validation: {reopened:?}"
    );
}

#[test]
fn test_owner_policy_transition_reports_policy_and_exact_modes() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 64)
        .expect("payload");

    for (directory_mode, file_mode) in [(0o700, 0o600), (0o500, 0o400), (0o700, 0o600)] {
        let policy = LocalOwnerAccessPolicy::new().with_unix_modes(directory_mode, file_mode);
        let outcome = stage
            .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
            .expect("apply exact owner policy");
        assert_eq!(
            qubit_local_files::outcome::LocalTreeAccessState::OwnerPolicyApplied,
            outcome.access_state()
        );
        assert_eq!(Some(&policy), outcome.owner_access_policy());
        let root = scratch.path().join(stage.relative_name());
        assert_eq!(
            directory_mode,
            fs::metadata(root).expect("root metadata").mode() & 0o777
        );
        assert_eq!(
            file_mode,
            fs::metadata(scratch.path().join(stage.relative_name()).join("payload"))
                .expect("payload metadata")
                .mode()
                & 0o777
        );
    }
    stage.cleanup().expect("cleanup");
}

#[test]
fn test_invalid_owner_policy_is_rejected_before_stage_creation() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let invalid = LocalOwnerAccessPolicy::new().with_unix_modes(0o770, 0o600);
    let error = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(invalid))
        .expect_err("group permissions are outside the owner policy contract");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::InvalidOptions,
        error.cause().kind()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeMutationState::NotStarted,
        error.mutation_state()
    );
    assert_eq!(0, fs::read_dir(scratch.path()).expect("parent contents").count());
}

#[cfg(feature = "test-support")]
#[test]
fn test_owner_policy_failure_on_second_child_reports_mixed_and_retains_policy() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("a"), &mut &b"a"[..], 8)
        .expect("first file");
    stage
        .write_file_new(Path::new("b"), &mut &b"b"[..], 8)
        .expect("second file");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let _fault = install_test_fault("owner-policy-apply").expect("fault selector");
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("second child application should fail");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Mixed,
        error.access_state()
    );
    assert_eq!(Some(&policy), stage.owner_access_policy());
    drop(error);
    stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect("retry should apply and verify the configured policy");
    stage.cleanup().expect("cleanup after explicit retry");
}

#[cfg(feature = "test-support")]
#[test]
fn test_owner_policy_identity_replacement_prevents_chmod() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"original"[..], 32)
        .expect("payload");
    let path = scratch.path().join(stage.relative_name()).join("payload");
    let mut backup_name = path.as_os_str().to_os_string();
    backup_name.push(".owner-policy-original");
    let backup = std::path::PathBuf::from(backup_name);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let _fault = install_test_fault("owner-policy-replace-before-open").expect("replacement selector");
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("replacement identity must be detected before applying permissions");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ChangedDuringRead,
        error.cause().kind()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Preserved,
        error.access_state()
    );
    drop(error);
    fs::remove_file(&path).expect("remove injected replacement");
    fs::rename(&backup, &path).expect("restore original identity");
    stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect("original object remains eligible for policy application");
    stage.cleanup().expect("cleanup");
}

#[cfg(feature = "test-support")]
#[test]
fn test_stage_identity_replacement_prevents_root_policy_mutation() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let native = scratch.path().join(stage.relative_name());
    let mut backup_name = native.as_os_str().to_os_string();
    backup_name.push(".owner-policy-original");
    let backup = std::path::PathBuf::from(backup_name);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let _fault = install_test_fault("owner-policy-replace-stage-before-root").expect("replacement selector");
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("replaced stage name must block root policy mutation");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ChangedDuringRead,
        error.cause().kind()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Mixed,
        error.access_state()
    );
    drop(error);
    fs::remove_dir(&native).expect("remove injected replacement stage");
    fs::rename(&backup, &native).expect("restore original stage identity");
    stage.cleanup().expect("cleanup restored stage");
}

#[cfg(feature = "test-support")]
#[test]
fn test_owner_policy_final_handle_check_detects_earlier_mode_change() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let _fault = install_test_fault("owner-policy-mutate-before-final-verify").expect("mode change selector");
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("the final retained-handle verification must catch later mode changes");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Indeterminate,
        error.access_state()
    );
    assert_eq!(Some(&policy), stage.owner_access_policy());
    assert_eq!(
        0o600,
        fs::metadata(scratch.path().join(stage.relative_name()).join("payload"))
            .expect("mutated payload")
            .mode()
            & 0o777
    );
    drop(error);
    stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect("explicit retry restores exact policy");
    stage.cleanup().expect("cleanup after retry");
}

#[cfg(feature = "test-support")]
#[test]
fn test_owner_policy_final_check_detects_replaced_processed_entry() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"original"[..], 32)
        .expect("payload");
    let path = scratch.path().join(stage.relative_name()).join("payload");
    let mut backup_name = path.as_os_str().to_os_string();
    backup_name.push(".owner-policy-original");
    let backup = std::path::PathBuf::from(backup_name);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let _fault = install_test_fault("owner-policy-replace-before-final-verify").expect("replacement selector");
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("final pathname binding must match the retained entry identity");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Indeterminate,
        error.access_state()
    );
    assert_eq!(Some(&policy), stage.owner_access_policy());
    drop(error);
    fs::remove_file(&path).expect("remove injected replacement");
    fs::rename(&backup, &path).expect("restore original object");
    stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect("explicit retry restores exact tree policy");
    stage.cleanup().expect("cleanup");
}

#[cfg(feature = "test-support")]
#[test]
fn test_owner_policy_final_enumeration_detects_new_root_entry() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let added = scratch
        .path()
        .join(stage.relative_name())
        .join("added-during-conversion");
    let _fault =
        install_test_fault("owner-policy-add-root-entry-before-final-verify").expect("root insertion selector");
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("final enumeration must include entries inserted after the preflight snapshot");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Indeterminate,
        error.access_state()
    );
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ChangedDuringRead,
        error.cause().kind()
    );
    assert!(added.exists(), "injected entry should remain available for recovery");
    drop(error);
    stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect("explicit retry applies policy to the complete current tree");
    stage.cleanup().expect("cleanup recovered tree");
}

#[cfg(feature = "test-support")]
#[test]
fn test_owner_policy_final_enumeration_detects_new_nested_entry() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
        .expect("stage");
    stage
        .write_file_new(Path::new("nested/seed"), &mut &b"seed"[..], 32)
        .expect("nested seed");
    let added = scratch
        .path()
        .join(stage.relative_name())
        .join("nested/added-during-conversion");
    let _fault =
        install_test_fault("owner-policy-add-nested-entry-before-final-verify").expect("nested insertion selector");
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("recursive final enumeration must include nested insertions");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Indeterminate,
        error.access_state()
    );
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ChangedDuringRead,
        error.cause().kind()
    );
    assert!(
        added.exists(),
        "injected nested entry should remain available for recovery"
    );
    drop(error);
    stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect("explicit retry applies policy to the complete nested tree");
    stage.cleanup().expect("cleanup recovered tree");
}

#[test]
fn test_publish_conflict_retains_stage_with_owner_policy() {
    use std::ffi::OsStr;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::policy::LocalTreeFinalAccess;

    let scratch = tempfile::tempdir().expect("scratch directory");
    fs::write(scratch.path().join("target"), b"existing").expect("conflict target");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("target"),
            &LocalTreePublishOptions::new().with_owner_access_policy(policy),
        )
        .expect_err("no-replace publication must retain stage on conflict");
    assert_eq!(
        qubit_local_files::outcome::LocalTreePublication::NotPublished,
        error.publication()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::OwnerPolicyApplied,
        error.access_state()
    );
    assert_eq!(Some(&policy), error.owner_access_policy());
    let mut stage = error.into_stage().expect("conflicted stage retained");
    assert_eq!(Some(&policy), stage.owner_access_policy());
    stage
        .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
        .expect("restore cleanup access");
    stage.cleanup().expect("cleanup retained stage");
}

#[cfg(feature = "test-support")]
#[test]
fn test_required_sync_failure_retains_stage_and_policy_before_conversion() {
    use std::ffi::OsStr;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::policy::LocalFileSyncRequirement;
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let _fault = install_test_fault("owner-policy-sync-failure").expect("sync selector");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Required),
        )
        .expect_err("required file sync failure must block policy transition and publication");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeMutationState::NotStarted,
        error.mutation_state()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Preserved,
        error.access_state()
    );
    assert_eq!(Some(&policy), error.owner_access_policy());
    let stage = error.into_stage().expect("stage retained");
    assert_eq!(Some(&policy), stage.owner_access_policy());
    stage.cleanup().expect("cleanup retained stage");
}

#[cfg(all(feature = "test-support", target_os = "linux"))]
#[test]
fn test_required_owner_policy_final_sync_failure_retains_changed_stage() {
    use std::ffi::OsStr;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreeAccessState;
    use qubit_local_files::outcome::LocalTreeMutationState;
    use qubit_local_files::outcome::LocalTreePublication;
    use qubit_local_files::policy::LocalFileSyncRequirement;
    use qubit_local_files::policy::LocalTreeFinalAccess;
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let native = scratch.path().join(stage.relative_name());
    let _fault = install_test_fault("owner-policy-final-sync-failure").expect("final sync selector");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Required),
        )
        .expect_err("required final sync failure must prevent publication");
    assert_eq!(LocalTreeMutationState::Started, error.mutation_state());
    assert_eq!(LocalTreeAccessState::OwnerPolicyApplied, error.access_state());
    assert_eq!(LocalTreePublication::NotPublished, error.publication());
    assert_eq!(qubit_local_files::error::LocalFileErrorKind::Io, error.cause().kind());
    assert!(!scratch.path().join("published").exists());
    let mut stage = error.into_stage().expect("changed stage retained");
    assert_eq!(0o500, fs::metadata(&native).expect("stage metadata").mode() & 0o777);
    assert_eq!(
        0o400,
        fs::metadata(native.join("payload")).expect("payload metadata").mode() & 0o777
    );
    stage
        .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
        .expect("restore cleanup access");
    stage.cleanup().expect("cleanup retained stage");
}

#[cfg(all(feature = "test-support", target_os = "linux"))]
#[test]
fn test_preferred_owner_policy_final_sync_failure_publishes_unsynced() {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreePublication;
    use qubit_local_files::policy::LocalFileSyncRequirement;
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let _fault = install_test_fault("owner-policy-final-sync-failure").expect("final sync selector");
    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Preferred),
        )
        .expect("preferred final sync failure may publish");
    assert_eq!(LocalTreePublication::Published, outcome.publication());
    assert!(!outcome.file_synced());
    assert_eq!(
        0o500,
        fs::metadata(scratch.path().join("published"))
            .expect("published metadata")
            .mode()
            & 0o777
    );
    assert_eq!(
        0o400,
        fs::metadata(scratch.path().join("published/payload"))
            .expect("payload metadata")
            .mode()
            & 0o777
    );
    fs::set_permissions(scratch.path().join("published"), fs::Permissions::from_mode(0o700))
        .expect("restore fixture root mode");
    fs::set_permissions(
        scratch.path().join("published/payload"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("restore fixture file mode");
}

#[cfg(all(feature = "test-support", target_os = "linux"))]
#[test]
fn test_required_owner_policy_sync_capability_failure_preserves_access() {
    use std::ffi::OsStr;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreeAccessState;
    use qubit_local_files::outcome::LocalTreeMutationState;
    use qubit_local_files::policy::LocalFileSyncRequirement;
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let native = scratch.path().join(stage.relative_name());
    let _fault = install_test_fault("owner-policy-sync-capability").expect("capability selector");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Required),
        )
        .expect_err("required sync capability must be established before access mutation");
    assert_eq!(LocalTreeMutationState::NotStarted, error.mutation_state());
    assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::Unsupported,
        error.cause().kind()
    );
    assert_eq!(
        0o600,
        fs::metadata(native.join("payload")).expect("payload metadata").mode() & 0o777
    );
    error
        .into_stage()
        .expect("unchanged stage retained")
        .cleanup()
        .expect("cleanup");
}

#[cfg(all(feature = "test-support", target_os = "linux"))]
#[test]
fn test_preferred_owner_policy_sync_capability_failure_publishes_unsynced() {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreePublication;
    use qubit_local_files::policy::LocalFileSyncRequirement;
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let _fault = install_test_fault("owner-policy-sync-capability").expect("capability selector");
    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Preferred),
        )
        .expect("preferred sync capability failure may publish");
    assert_eq!(LocalTreePublication::Published, outcome.publication());
    assert!(!outcome.file_synced());
    fs::set_permissions(scratch.path().join("published"), fs::Permissions::from_mode(0o700))
        .expect("restore fixture root mode");
    fs::set_permissions(
        scratch.path().join("published/payload"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("restore fixture file mode");
}

#[cfg(target_os = "linux")]
#[test]
fn test_required_owner_policy_sync_accepts_empty_tree() {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreePublication;
    use qubit_local_files::policy::LocalFileSyncRequirement;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0, 0);
    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_file_sync(LocalFileSyncRequirement::Required),
        )
        .expect("empty tree has no regular files requiring sync");
    assert_eq!(LocalTreePublication::Published, outcome.publication());
    assert!(outcome.file_synced());
    fs::set_permissions(scratch.path().join("published"), fs::Permissions::from_mode(0o700))
        .expect("restore fixture root mode");
}

#[cfg(feature = "test-support")]
#[test]
fn test_publish_rechecks_source_identity_after_policy_conversion() {
    use std::ffi::OsStr;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreePublication;
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let native = scratch.path().join(stage.relative_name());
    let mut backup_name = native.as_os_str().to_os_string();
    backup_name.push(".owner-policy-original");
    let backup = std::path::PathBuf::from(backup_name);
    let _fault = install_test_fault("owner-policy-replace-source-before-rename").expect("source replacement selector");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_owner_access_policy(policy),
        )
        .expect_err("stage source replacement must block the rename");
    assert_eq!(LocalTreePublication::NotPublished, error.publication());
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ChangedDuringRead,
        error.cause().kind()
    );
    assert!(
        parent
            .entry_metadata(Path::new("published"))
            .expect("target query")
            .is_none()
    );
    let stage = error.into_stage().expect("original stage authority retained");
    fs::remove_dir(&native).expect("remove replacement source");
    fs::rename(&backup, &native).expect("restore original source binding");
    stage.cleanup().expect("cleanup original stage");
}

#[test]
fn test_cleanup_failure_retains_owner_policy_for_recovery() {
    use qubit_local_files::options::LocalTempCleanupLimits;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0));
    let error = stage
        .cleanup()
        .expect_err("zero cleanup budget should retain recovery owner");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeMutationState::NotStarted,
        error.mutation_state()
    );
    assert_eq!(Some(&policy), error.owner_access_policy());
    let mut resources = error.into_retained_resources();
    let mut resource = resources.pop().expect("cleanup recovery owner");
    resource.set_cleanup_limits(LocalTempCleanupLimits::new());
    resource.cleanup().expect("unbounded retry cleans the stage");
}

#[test]
fn test_empty_stage_accepts_zero_mode_root_policy() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let zero = LocalOwnerAccessPolicy::new().with_unix_modes(0, 0);
    stage
        .apply_owner_access_policy_with_options(&zero, &qubit_local_files::options::LocalListOptions::new())
        .expect("pre-opened empty stage can verify root policy without rewalking");
    assert_eq!(Some(&zero), stage.owner_access_policy());
    let native = scratch.path().join(stage.relative_name());
    assert_eq!(0, fs::metadata(&native).expect("root metadata").mode() & 0o777);
    drop(stage.retain_in_place());
    fs::remove_dir(native).expect("empty mode-zero root can be removed from its parent");
}

#[test]
fn test_nonempty_stage_rejects_unsearchable_root_policy_before_mutation() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0, 0o400);
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("unsearchable nonempty root cannot support final name-binding verification");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::RequirementNotMet,
        error.cause().kind()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Preserved,
        error.access_state()
    );
    assert_eq!(Some(&policy), stage.owner_access_policy());
    assert_eq!(
        0o700,
        fs::metadata(scratch.path().join(stage.relative_name()))
            .expect("root metadata")
            .mode()
            & 0o777
    );
    assert_eq!(
        0o600,
        fs::metadata(scratch.path().join(stage.relative_name()).join("payload"))
            .expect("file metadata")
            .mode()
            & 0o777
    );
    stage.cleanup().expect("cleanup untouched tree");
}

#[cfg(target_os = "linux")]
#[test]
fn test_owner_policy_depth_zero_rejects_before_permission_changes() {
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::outcome::LocalTreeAccessState;

    let scratch = tempfile::tempdir().expect("scratch");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 64)
        .expect("payload");
    let native = scratch.path().join(stage.relative_name()).join("payload");
    fs::set_permissions(&native, fs::Permissions::from_mode(0o600)).expect("mode");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new().with_max_depth(0))
        .expect_err("nonempty tree exceeds depth zero");
    assert_eq!(error.cause().kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.access_state(), LocalTreeAccessState::Preserved);
    assert_eq!(fs::metadata(&native).expect("metadata").mode() & 0o777, 0o600);
    stage.cleanup().expect("cleanup");
}

#[cfg(target_os = "linux")]
#[test]
fn test_owner_policy_depth_zero_accepts_empty_tree() {
    use qubit_local_files::options::LocalListOptions;

    let scratch = tempfile::tempdir().expect("scratch");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0, 0);
    stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new().with_max_depth(0))
        .expect("depth-zero empty tree");
    assert_eq!(
        0,
        fs::metadata(scratch.path().join(stage.relative_name()))
            .expect("root metadata")
            .mode()
            & 0o777
    );
    let native = scratch.path().join(stage.relative_name());
    drop(stage.retain_in_place());
    fs::remove_dir(native).expect("remove empty zero-mode stage from parent");
}

#[cfg(target_os = "linux")]
#[test]
fn test_owner_policy_depth_one_accepts_direct_child() {
    use qubit_local_files::options::LocalListOptions;

    let scratch = tempfile::tempdir().expect("scratch");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 64)
        .expect("payload");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new().with_max_depth(1))
        .expect("one-level tree fits depth one");
    assert_eq!(
        0o400,
        fs::metadata(scratch.path().join(stage.relative_name()).join("payload"))
            .expect("payload metadata")
            .mode()
            & 0o777
    );
    stage.cleanup().expect("cleanup");
}

#[cfg(target_os = "linux")]
#[test]
fn test_owner_policy_depth_one_rejects_nested_leaf_before_permission_changes() {
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::options::LocalCreateDirectoryOptions;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::outcome::LocalTreeAccessState;

    let scratch = tempfile::tempdir().expect("scratch");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let _nested = stage
        .directory()
        .create_directory_with_options(Path::new("nested"), &LocalCreateDirectoryOptions::new())
        .expect("nested directory");
    stage
        .write_file_new(Path::new("nested/payload"), &mut &b"payload"[..], 64)
        .expect("nested payload");
    let native = scratch.path().join(stage.relative_name());
    fs::set_permissions(native.join("nested"), fs::Permissions::from_mode(0o700)).expect("nested mode");
    fs::set_permissions(native.join("nested/payload"), fs::Permissions::from_mode(0o600)).expect("payload mode");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new().with_max_depth(1))
        .expect_err("nested leaf exceeds depth one");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit,
        error.cause().kind()
    );
    assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
    assert_eq!(
        0o700,
        fs::metadata(native.join("nested")).expect("nested metadata").mode() & 0o777
    );
    assert_eq!(
        0o600,
        fs::metadata(native.join("nested/payload"))
            .expect("payload metadata")
            .mode()
            & 0o777
    );
    stage.cleanup().expect("cleanup");
}

#[test]
fn test_owner_policy_handle_budget_rejects_before_any_mutation() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    for index in 0..257 {
        stage
            .write_file_new(
                &Path::new("payload").with_file_name(format!("payload-{index}")),
                &mut &b"payload"[..],
                32,
            )
            .expect("entry creation");
    }
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &qubit_local_files::options::LocalListOptions::new())
        .expect_err("more entries than the retained-handle budget must fail in preflight");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit,
        error.cause().kind()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::Preserved,
        error.access_state()
    );
    assert_eq!(Some(&policy), stage.owner_access_policy());
    assert!(
        fs::read_dir(scratch.path().join(stage.relative_name()))
            .expect("stage entries")
            .all(|entry| { entry.expect("entry").metadata().expect("metadata").mode() & 0o777 == 0o600 })
    );
    stage.cleanup().expect("cleanup unchanged tree");
}

#[test]
fn test_publish_success_reports_applied_owner_policy() {
    use std::ffi::OsStr;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreeAccessState;
    use qubit_local_files::outcome::LocalTreePublication;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");
    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_owner_access_policy(policy),
        )
        .expect("publish with exact owner policy");
    assert_eq!(LocalTreePublication::Published, outcome.publication());
    assert_eq!(LocalTreeAccessState::OwnerPolicyApplied, outcome.access_state());
    assert_eq!(Some(&policy), outcome.owner_access_policy());
    assert_eq!(
        0o400,
        fs::metadata(scratch.path().join("published/payload"))
            .expect("published payload")
            .mode()
            & 0o777
    );
}

#[test]
fn test_publish_owner_policy_conflict_is_rejected_before_mutation() {
    use std::ffi::OsStr;

    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::outcome::LocalTreeAccessState;
    use qubit_local_files::outcome::LocalTreeMutationState;
    use qubit_local_files::policy::LocalTreeFinalAccess;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(&scratch);
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_owner_access_policy(policy)
                .with_access(LocalTreeFinalAccess::CurrentUserOnly),
        )
        .expect_err("conflicting access configuration must be rejected");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::InvalidOptions,
        error.cause().kind()
    );
    assert_eq!(LocalTreeMutationState::NotStarted, error.mutation_state());
    assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
    let stage = error.into_stage().expect("stage retained");
    stage.cleanup().expect("untouched stage remains cleanable");
    assert!(
        parent
            .entry_metadata(Path::new("published"))
            .expect("target query")
            .is_none()
    );
}
