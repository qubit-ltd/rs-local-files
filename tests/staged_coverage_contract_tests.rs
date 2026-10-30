// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(target_os = "linux")]

use std::ffi::OsStr;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::directory::LocalStagedDirectory;
use qubit_local_files::directory::LocalTreeEntry;
use qubit_local_files::directory::LocalTreeValidationOptions;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreePublishOptions;
use qubit_local_files::outcome::LocalFileKind;
use qubit_local_files::outcome::LocalTreeAccessState;
use qubit_local_files::outcome::LocalTreeMutationState;
use qubit_local_files::outcome::LocalTreePublication;
use qubit_local_files::policy::LocalFileSyncRequirement;
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalPrivacyRequirement;
use qubit_local_files::policy::LocalTreeFinalAccess;

fn parent(path: &Path) -> LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(path, &LocalDirectoryOpenOptions::new())
        .expect("parent authority")
}

fn stage(parent: &LocalDirectoryHandle, options: &LocalTempDirectoryOptions) -> LocalStagedDirectory {
    parent.create_staged_directory(options).expect("staged directory")
}

fn write(stage: &mut LocalStagedDirectory, name: &str) {
    stage
        .write_file_new(Path::new(name), &mut &b"payload"[..], 7)
        .expect("write exact byte limit");
}

struct FailingInput(bool);

impl io::Read for FailingInput {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.0 {
            return Err(io::Error::other("scripted input failure"));
        }
        self.0 = true;
        bytes[..7].copy_from_slice(b"partial");
        Ok(7)
    }
}

#[test]
fn test_write_errors_remove_partial_files_and_allow_retry() {
    for options in [
        LocalTempDirectoryOptions::new(),
        LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        LocalTempDirectoryOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600)),
    ] {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let parent = parent(scratch.path());
        let mut stage = stage(&parent, &options);
        let path = scratch.path().join(stage.relative_name());
        let error = stage
            .write_file_new(Path::new("partial"), &mut FailingInput(false), 64)
            .expect_err("input fails after initial bytes");
        assert_eq!(LocalFileErrorKind::Io, error.kind());
        assert_eq!(LocalFileOperation::Read, error.operation());
        assert!(
            !path.join("partial").exists(),
            "failed stream must leave no partial file"
        );
        let error = stage
            .write_file_new(Path::new("limited"), &mut &b"too long"[..], 0)
            .expect_err("zero byte limit rejects nonempty input");
        assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
        assert!(!path.join("limited").exists());
        write(&mut stage, "partial");
        write(&mut stage, "limited");
        let error = stage
            .write_file_new(Path::new("partial"), &mut &b"replacement"[..], 64)
            .expect_err("create-new preserves existing contents");
        assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
        assert_eq!(
            b"payload",
            std::fs::read(path.join("partial"))
                .expect("original contents")
                .as_slice()
        );
        stage.cleanup().expect("cleanup tracks successful retries");
        assert!(!path.exists());
    }
}

#[test]
fn test_owner_write_reuses_policy_checked_parent_and_rejects_wrong_mode() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let options = LocalTempDirectoryOptions::new()
        .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600));
    let mut stage = stage(&parent, &options);
    write(&mut stage, "nested/deep/first");
    write(&mut stage, "nested/deep/second");
    let path = scratch.path().join(stage.relative_name());
    std::fs::set_permissions(path.join("nested"), std::fs::Permissions::from_mode(0o755))
        .expect("change parent away from exact owner policy");
    let error = stage
        .write_file_new(Path::new("nested/third"), &mut &b"data"[..], 4)
        .expect_err("existing parent must match exact policy");
    assert_eq!(LocalFileErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalFileOperation::Metadata, error.operation());
    assert!(!path.join("nested/third").exists());
    std::fs::set_permissions(path.join("nested"), std::fs::Permissions::from_mode(0o700))
        .expect("restore fixture parent mode");
    stage.cleanup().expect("cleanup nested stage");
}

#[test]
fn test_owner_write_rejects_file_parent_and_invalid_relative_paths() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let mut stage = stage(
        &parent,
        &LocalTempDirectoryOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600)),
    );
    write(&mut stage, "file");
    let error = stage
        .write_file_new(Path::new("file/child"), &mut &b"data"[..], 4)
        .expect_err("ordinary file cannot be a parent directory");
    assert_eq!(LocalFileErrorKind::NotDirectory, error.kind());
    for name in ["../escape", "/absolute"] {
        let error = stage
            .write_file_new(Path::new(name), &mut &b"data"[..], 4)
            .expect_err("stage rejects escape paths");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    }
}

#[test]
fn test_sync_budget_errors_leave_stage_owned_and_unpublished() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
    let path = scratch.path().join(stage.relative_name());
    std::fs::create_dir(path.join("nested")).expect("nested directory");
    write(&mut stage, "nested/payload");
    for limits in [
        LocalListOptions::new().with_max_depth(1),
        LocalListOptions::new().with_max_entries(0),
        LocalListOptions::new().with_max_seen_name_bytes(0),
    ] {
        assert!(
            !stage
                .sync_files_with_options(&limits, LocalFileSyncRequirement::NotRequired)
                .expect("disabled sync performs no traversal")
        );
        let error = stage
            .sync_files_with_options(&limits, LocalFileSyncRequirement::Required)
            .expect_err("required sync enforces traversal budget");
        assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
        let error = stage
            .sync_tree_with_options(&parent, &limits)
            .expect_err("tree sync enforces traversal budget");
        assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    }
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("stage remains usable after budget errors");
    stage.cleanup().expect("cleanup unpublished stage");
    assert!(!path.exists());
}

#[test]
fn test_sync_tree_rejects_different_stage_identity_under_supplied_parent() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let other = tempfile::tempdir().expect("other parent");
    let parent = parent(scratch.path());
    let stage = stage(&parent, &LocalTempDirectoryOptions::new());
    std::fs::create_dir(other.path().join(stage.relative_name())).expect("unrelated same-name directory");
    let error = stage
        .sync_tree_with_options(&self::parent(other.path()), &LocalListOptions::new())
        .expect_err("same name does not establish stage identity");
    assert_eq!(LocalFileErrorKind::ChangedDuringRead, error.kind());
    assert_eq!(LocalFileOperation::Metadata, error.operation());
}

#[test]
fn test_verify_entries_rejects_conflicting_expected_sets() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let stage = stage(&parent, &LocalTempDirectoryOptions::new());
    let configured = LocalTreeEntry::new("payload".into(), LocalFileKind::File).expect("valid entry");
    let error = stage
        .verify_entries(
            &[],
            &LocalTreeValidationOptions::new().with_exact_entries(vec![configured]),
        )
        .expect_err("two expected sets must agree");
    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
    assert_eq!(LocalFileOperation::ValidateName, error.operation());
}

#[test]
fn test_publish_rejects_invalid_names_and_returns_usable_stage() {
    for name in ["", ".", "..", "nested/target", "/absolute"] {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let parent = parent(scratch.path());
        let stage = stage(&parent, &LocalTempDirectoryOptions::new());
        let error = stage
            .publish_new(&parent, OsStr::new(name), &LocalTreePublishOptions::new())
            .expect_err("publication requires a single normal component");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.cause().kind());
        assert_eq!(LocalTreeMutationState::NotStarted, error.mutation_state());
        assert_eq!(LocalTreePublication::NotPublished, error.publication());
        assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
        error.into_stage().expect("recover stage").cleanup().expect("cleanup");
    }
}

#[test]
fn test_publish_rejects_wrong_parent_and_validation_before_mutating_access() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let other = tempfile::tempdir().expect("other parent");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
    write(&mut stage, "payload");
    let error = stage
        .publish_new(
            &self::parent(other.path()),
            OsStr::new("target"),
            &LocalTreePublishOptions::new(),
        )
        .expect_err("cannot publish into a different parent authority");
    assert_eq!(LocalFileErrorKind::InvalidState, error.cause().kind());
    assert_eq!(LocalFileOperation::Rename, error.cause().operation());
    let stage = error.into_stage().expect("recover stage");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("target"),
            &LocalTreePublishOptions::new()
                .with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser)
                .with_validation(LocalTreeValidationOptions::new().with_exact_entries(Vec::new())),
        )
        .expect_err("unexpected payload violates exact set");
    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.cause().kind());
    assert_eq!(LocalTreeMutationState::NotStarted, error.mutation_state());
    assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
    assert!(!scratch.path().join("target").exists());
    error
        .into_stage()
        .expect("recover validated stage")
        .cleanup()
        .expect("cleanup");
}

#[test]
fn test_access_policies_preserve_symlink_targets() {
    for owner_policy in [false, true] {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let parent = parent(scratch.path());
        let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
        let target = scratch.path().join("outside");
        std::fs::write(&target, b"outside").expect("outside payload");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).expect("outside mode");
        std::os::unix::fs::symlink(&target, scratch.path().join(stage.relative_name()).join("link"))
            .expect("stage link");
        if owner_policy {
            let error = stage
                .apply_owner_access_policy_with_options(
                    &LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600),
                    &LocalListOptions::new(),
                )
                .expect_err("exact owner policy rejects symbolic links");
            assert_eq!(LocalFileErrorKind::RequirementNotMet, error.cause().kind());
            assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
        } else {
            let outcome = stage
                .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
                .expect("current-user access skips links");
            assert_eq!(LocalTreeAccessState::CurrentUserOnly, outcome.access_state());
        }
        assert_eq!(
            0o644,
            std::fs::metadata(&target)
                .expect("outside metadata")
                .permissions()
                .mode()
                & 0o777
        );
        assert_eq!(b"outside", std::fs::read(&target).expect("outside data").as_slice());
    }
}
