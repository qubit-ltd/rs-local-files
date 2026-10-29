// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::directory::LocalTreeValidationOptions;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::observation::LocalSourceVerification;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::outcome::LocalTreeMutationState;
use qubit_local_files::policy::LocalPrivacyRequirement;
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::TempDir;

fn fixture(binding: bool) -> (TempDir, LocalDirectoryHandle) {
    let scratch = tempfile::tempdir().expect("fixture should exist");
    fs::create_dir_all(scratch.path().join("parent/child")).expect("nested fixture should exist");
    fs::write(scratch.path().join("parent/payload"), b"retained payload").expect("payload should exist");
    let mut options = LocalDirectoryOpenOptions::new();
    if binding {
        options = options.with_path_binding(LocalPathBindingOptions::new());
    }
    let handle = LocalFileSystem::rooted(scratch.path())
        .expect("filesystem should open")
        .open_directory_with_options(Path::new("parent"), &options)
        .expect("parent authority should open");
    (scratch, handle)
}

#[test]
fn test_ancestor_collection_requires_binding_and_enforces_each_budget() {
    let (_scratch, unbound) = fixture(false);
    let error = unbound
        .ancestor_observations(&LocalListOptions::new())
        .expect_err("unbound handles cannot supply opening observations");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidState);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);

    let (_scratch, bound) = fixture(true);
    let observations = bound
        .ancestor_observations(&LocalListOptions::new())
        .expect("bound observations should be available");
    assert_eq!(observations.len(), 2);
    assert_eq!(
        observations
            .last()
            .expect("directory observation should exist")
            .identity(),
        bound
            .observe()
            .expect("retained authority should be observable")
            .identity()
    );
    for options in [
        LocalListOptions::new().with_max_depth(1),
        LocalListOptions::new().with_max_entries(1),
        LocalListOptions::new().with_max_seen_name_bytes(0),
        LocalListOptions::new().with_max_open_directories(1),
        LocalListOptions::new().with_deadline(Duration::ZERO),
    ] {
        let error = bound
            .ancestor_observations(&options)
            .expect_err("each independent collection budget must apply");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit, "{options:?}");
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
    }
    assert_eq!(
        bound
            .ancestor_observations(&LocalListOptions::new().with_max_entries(2))
            .expect("the exact entry limit should succeed")
            .len(),
        2
    );
}

#[test]
fn test_child_binding_counts_inherited_ancestors_before_accepting_limit() {
    let (scratch, parent) = fixture(true);
    let error = parent
        .open_directory(
            Path::new("child"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_ancestors(1)),
        )
        .expect_err("child-local budget must also account for inherited ancestors");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
    assert_eq!(error.path(), Some(scratch.path().join("parent/child").as_path()));

    let child = parent
        .open_directory(
            Path::new("child"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_ancestors(2)),
        )
        .expect("exact ancestor limit should permit the child");
    let ancestors = child
        .ancestor_observations(&LocalListOptions::new())
        .expect("child should preserve inherited binding observations");
    assert_eq!(ancestors.len(), 3);
    assert_eq!(
        ancestors[1].identity(),
        parent.observe().expect("parent should remain observable").identity()
    );
}

#[test]
fn test_observe_entry_rejects_empty_entry_and_retains_missing_context() {
    let (scratch, handle) = fixture(false);
    for relative in [Path::new(""), Path::new("."), Path::new("child/..")] {
        let error = handle.observe_entry(relative).expect_err("an entry name is required");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
    }
    let error = handle
        .observe_entry(Path::new("child/missing"))
        .expect_err("missing entries cannot supply observations");
    assert_eq!(error.kind(), LocalFileErrorKind::NotFound);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(
        error.path(),
        Some(scratch.path().join("parent/child/missing").as_path())
    );
    assert!(
        handle
            .entry_metadata(Path::new("child/missing"))
            .expect("absence should be safe")
            .is_none()
    );
}

#[test]
fn test_directory_open_rejects_follow_across_scope_on_both_routes() {
    let (scratch, handle) = fixture(false);
    let options = LocalDirectoryOpenOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope);
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("filesystem should open");
    for result in [
        filesystem.open_directory_with_options(Path::new("parent"), &options),
        handle.open_directory(Path::new("child"), &options),
    ] {
        let error = result.expect_err("fixed directory authority must reject cross-scope policy");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
    }
}

#[cfg(unix)]
#[test]
fn test_binding_reports_missing_and_changed_while_reader_keeps_authority() {
    let (scratch, handle) = fixture(true);
    let mut reader = handle
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("bound reader should open");
    assert_eq!(
        reader.verify_path_binding().expect("original binding should verify"),
        LocalSourceVerification::Unchanged
    );
    fs::rename(scratch.path().join("parent"), scratch.path().join("retained"))
        .expect("fixture authority should be renamed");
    assert_eq!(
        reader
            .verify_path_binding()
            .expect("missing pathname is a verification result"),
        LocalSourceVerification::Missing
    );
    fs::create_dir(scratch.path().join("parent")).expect("replacement directory should exist");
    assert_eq!(
        reader
            .verify_path_binding()
            .expect("replacement identity should be compared"),
        LocalSourceVerification::Changed
    );
    assert_eq!(
        reader
            .read_to_end_limited(64)
            .expect("reader must retain the opened file"),
        b"retained payload"
    );
}

#[cfg(unix)]
#[test]
fn test_binding_propagates_non_directory_replacement_with_anchor_context() {
    let (scratch, handle) = fixture(true);
    let reader = handle
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("bound reader should open");
    let original = scratch.path().join("parent");
    fs::rename(&original, scratch.path().join("retained")).expect("fixture directory should be retained");
    fs::write(&original, b"replacement file").expect("replacement file should exist");
    let error = reader
        .verify_path_binding()
        .expect_err("a non-directory anchor replacement is an error");
    assert_eq!(error.kind(), LocalFileErrorKind::NotDirectory);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(original.as_path()));
}

#[test]
fn test_failed_stage_identity_validation_preserves_named_directory() {
    let (scratch, handle) = fixture(false);
    let wrong_identity = handle
        .observe()
        .expect("parent should be observable")
        .identity()
        .expect("native identity should exist")
        .clone();
    let error = handle
        .open_staged_directory(Path::new("child"), &wrong_identity, &LocalTreeValidationOptions::new())
        .expect_err("a parent identity cannot authorize reopening its child");
    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(scratch.path().join("parent/child").as_path()));
    assert!(scratch.path().join("parent/child").is_dir());
}

#[cfg(unix)]
#[test]
fn test_private_stage_reopen_rejects_group_access_without_removing_stage() {
    use std::os::unix::fs::PermissionsExt;

    let (scratch, handle) = fixture(false);
    let child_path = scratch.path().join("parent/child");
    fs::set_permissions(&child_path, fs::Permissions::from_mode(0o750))
        .expect("privacy fixture mode should be explicit");
    let identity = handle
        .observe_entry(Path::new("child"))
        .expect("child should be observed")
        .identity()
        .expect("native identity should exist")
        .clone();
    let error = handle
        .open_staged_directory(
            Path::new("child"),
            &identity,
            &LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect_err("group-readable stage cannot meet required privacy");
    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert!(child_path.is_dir());
    assert_eq!(
        fs::metadata(child_path)
            .expect("stage should remain")
            .permissions()
            .mode()
            & 0o777,
        0o750
    );
}

#[test]
fn test_zero_attempt_stage_creation_reports_no_mutation() {
    let (scratch, handle) = fixture(false);
    let error = handle
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_max_attempts(0))
        .expect_err("zero attempts cannot create a stage");
    assert_eq!(error.cause().kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.cause().operation(), LocalFileOperation::GenerateName);
    assert_eq!(error.mutation_state(), LocalTreeMutationState::NotStarted);
    assert!(error.retained_resources().is_empty());
    assert!(error.stage_name().is_none());
    assert_eq!(
        fs::read_dir(scratch.path().join("parent"))
            .expect("parent should remain readable")
            .count(),
        2
    );
}
