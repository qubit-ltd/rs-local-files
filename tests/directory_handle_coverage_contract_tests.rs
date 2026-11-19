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
#[cfg(unix)]
use qubit_local_files::observation::LocalSourceVerification;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
#[cfg(unix)]
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::outcome::LocalTreeMutationState;
#[cfg(unix)]
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
fn test_child_can_enable_binding_when_parent_handle_is_unbound() {
    let (_scratch, parent) = fixture(false);
    let child = parent
        .open_directory(
            Path::new("child"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_ancestors(1)),
        )
        .expect("child should establish binding from an unbound parent");

    let ancestors = child
        .ancestor_observations(&LocalListOptions::new())
        .expect("newly bound child should expose its retained observation chain");
    assert_eq!(ancestors.len(), 2);
    assert_eq!(
        ancestors[0].identity(),
        parent
            .observe()
            .expect("unbound parent should remain observable")
            .identity()
    );
    assert_eq!(
        ancestors[1].identity(),
        child.observe().expect("bound child should be observable").identity()
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

#[test]
fn test_public_directory_handle_operations_cover_success_and_resolution_errors() {
    use qubit_local_files::options::LocalCreateDirectoryOptions;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;

    let (scratch, handle) = fixture(false);

    let directory = handle
        .create_directory_with_options(
            Path::new("created/nested"),
            &LocalCreateDirectoryOptions::new().with_recursive(),
        )
        .expect("recursive directory creation should succeed");
    let _ = directory;
    assert!(scratch.path().join("parent/created/nested").is_dir());
    let _existing = handle
        .create_directory_with_options(
            Path::new("created/nested"),
            &LocalCreateDirectoryOptions::new().with_exists_ok(),
        )
        .expect("existing directory should be accepted when requested");

    let child = handle
        .open_directory(Path::new("child"), &LocalDirectoryOpenOptions::new())
        .expect("child directory should open through the public handle");
    assert_eq!(
        child
            .observe()
            .expect("child authority should be observable")
            .metadata()
            .kind(),
        qubit_local_files::outcome::LocalFileKind::Directory
    );

    for (path, expected, expected_operation) in [
        (
            Path::new("/absolute"),
            LocalFileErrorKind::InvalidPath,
            LocalFileOperation::BindPath,
        ),
        (
            Path::new("../outside"),
            LocalFileErrorKind::InvalidPath,
            LocalFileOperation::OpenRoot,
        ),
        (
            Path::new("absent"),
            LocalFileErrorKind::NotFound,
            LocalFileOperation::Metadata,
        ),
        (
            Path::new("payload/child"),
            LocalFileErrorKind::InvalidPath,
            LocalFileOperation::OpenRoot,
        ),
    ] {
        let error = handle
            .open_directory(path, &LocalDirectoryOpenOptions::new())
            .expect_err("invalid child paths should be rejected");
        assert_eq!(error.kind(), expected, "path: {path:?}");
        assert_eq!(error.operation(), expected_operation, "path: {path:?}");
    }

    fs::write(scratch.path().join("parent/copy-source"), b"copy me").expect("copy source should be created");
    let copied = handle
        .copy_with_options(
            Path::new("copy-source"),
            Path::new("copy-target"),
            &qubit_local_files::options::LocalCopyOptions::new(),
        )
        .expect("handle copy should complete");
    assert_eq!(copied.stats().files(), 1);
    assert_eq!(fs::read(scratch.path().join("parent/copy-target")).unwrap(), b"copy me");

    let missing = handle
        .copy_with_options(
            Path::new("missing-copy-source"),
            Path::new("copy-failure-target"),
            &qubit_local_files::options::LocalCopyOptions::new(),
        )
        .expect_err("missing handle copy source should preserve a typed failure");
    assert_eq!(missing.error().kind(), LocalFileErrorKind::NotFound);
    assert_eq!(missing.error().operation(), LocalFileOperation::Copy);
    assert!(!scratch.path().join("parent/copy-failure-target").exists());

    let error = handle
        .open_writer_with_options(
            Path::new("../escape"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("writer paths cannot escape the retained authority");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    assert_eq!(error.operation(), LocalFileOperation::BindPath);
    assert!(!scratch.path().join("escape").exists());
}

#[test]
fn test_public_directory_handle_listing_metadata_and_security_errors() {
    use qubit_local_files::observation::LocalSecurityObservationOptions;

    let (_scratch, handle) = fixture(false);
    assert!(
        handle
            .entry_metadata(Path::new("payload"))
            .expect("existing entry metadata should be available")
            .is_some()
    );

    let entries = handle
        .list_with_options(Path::new("."), &LocalListOptions::new())
        .expect("listing the authority root should succeed")
        .collect::<Result<Vec<_>, _>>()
        .expect("listing entries should be readable");
    assert!(
        entries
            .iter()
            .any(|entry| entry.relative_path() == Path::new("payload"))
    );

    let error = handle
        .list_with_options(Path::new("payload"), &LocalListOptions::new())
        .expect_err("a file cannot be listed as a directory");
    assert_eq!(error.kind(), LocalFileErrorKind::TypeConflict);
    assert_eq!(error.operation(), LocalFileOperation::List);

    let observation = handle
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("best effort security facts should be queryable");
    assert!(
        observation.identity().is_some()
            || observation.filesystem().is_some()
            || observation.access().is_some()
            || observation.link_count().is_some()
    );
}
