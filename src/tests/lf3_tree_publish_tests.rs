// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsStr;
use std::path::Path;

use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTreePublishOptions;
use crate::options::LocalTreeValidationOptions;
use crate::outcome::LocalFileKind;
use crate::outcome::LocalTreeAccessState;
use crate::outcome::LocalTreePublication;
use crate::policy::LocalTreeFinalAccess;

#[test]
fn lf_review_tree_exact_set_conflicts_leave_destination_and_owner_untouched() {
    for (name, expected) in [
        ("missing", Vec::new()),
        (
            "extra",
            vec![
                crate::directory::LocalTreeEntry::new(Path::new("other").to_path_buf(), LocalFileKind::File)
                    .expect("expected"),
            ],
        ),
        (
            "kind",
            vec![
                crate::directory::LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::Directory)
                    .expect("expected"),
            ],
        ),
    ] {
        let fixture = tempfile::tempdir().expect("fixture");
        let fs = crate::LocalFileSystem::host().expect("Host");
        let parent = fs
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        let mut bytes = b"x".as_slice();
        stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
        let destination = fixture.path().join(name);
        let error = stage
            .publish_new(
                &parent,
                OsStr::new(name),
                &LocalTreePublishOptions::new()
                    .with_validation(LocalTreeValidationOptions::new().with_exact_entries(expected)),
            )
            .expect_err("exact-set conflict must reject publication");
        assert_eq!(
            error.cause().kind(),
            crate::error::LocalFileErrorKind::RequirementNotMet
        );
        assert_eq!(error.publication(), LocalTreePublication::NotPublished);
        assert!(!destination.exists());
        error
            .into_stage()
            .expect("owner returned")
            .cleanup()
            .expect("stage cleanup");
    }
}

#[test]
fn lf_n02_required_sync_accepts_a_file_at_the_depth_boundary() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = crate::LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("file"), &mut &b"payload"[..], 16)
        .expect("file");
    let options = LocalTreePublishOptions::new()
        .with_validation(LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(1)))
        .with_file_sync(crate::policy::LocalFileSyncRequirement::Required);

    let outcome = stage
        .publish_new(&parent, OsStr::new("published"), &options)
        .expect("depth-one file should be validated and synced");
    assert_eq!(outcome.publication(), LocalTreePublication::Published);
    assert!(outcome.file_synced());
    assert_eq!(
        std::fs::read(fixture.path().join("published/file")).expect("published file"),
        b"payload"
    );
}

#[test]
fn lf_n02_over_depth_publish_is_not_published_and_returns_stage() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = crate::LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    std::fs::create_dir(fixture.path().join(stage.relative_name()).join("one")).expect("first directory");
    std::fs::create_dir(fixture.path().join(stage.relative_name()).join("one/two")).expect("second directory");
    stage
        .write_file_new(Path::new("one/two/file"), &mut &b"payload"[..], 16)
        .expect("nested file");
    let options = LocalTreePublishOptions::new()
        .with_validation(LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(2)))
        .with_file_sync(crate::policy::LocalFileSyncRequirement::Required);

    let error = stage
        .publish_new(&parent, OsStr::new("published"), &options)
        .expect_err("deep entry must prevent publication");
    assert_eq!(error.cause().kind(), crate::error::LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert!(!fixture.path().join("published").exists());
    error
        .into_stage()
        .expect("stage owner retained")
        .cleanup()
        .expect("stage cleanup");
}

#[test]
fn lf_review_tree_duplicate_expected_paths_preserve_publish_owner() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = crate::LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let entry =
        crate::directory::LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::File).expect("expected");
    let validation = LocalTreeValidationOptions::new().with_exact_entries(vec![entry.clone(), entry]);
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("destination"),
            &LocalTreePublishOptions::new().with_validation(validation),
        )
        .expect_err("duplicate expected paths are invalid");
    assert_eq!(error.cause().kind(), crate::error::LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert!(!fixture.path().join("destination").exists());
    error
        .into_stage()
        .expect("owner returned")
        .cleanup()
        .expect("stage cleanup");
}

#[cfg(unix)]
#[test]
fn lf_review_tree_special_allowed_with_only_limits() {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener;

    let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
    let root = std::fs::canonicalize(fixture.path()).expect("canonicalize fixture");
    let fs = crate::LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(&root, &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let source = root.join(stage.relative_name()).join("socket");
    drop(UnixListener::bind(&source).expect("socket"));
    let validation = LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_entries(4));
    let _ = stage
        .publish_new(
            &parent,
            OsStr::new("destination"),
            &LocalTreePublishOptions::new().with_validation(validation),
        )
        .expect("special entry allowed by default");
    assert!(
        std::fs::symlink_metadata(root.join("destination/socket"))
            .expect("published socket")
            .file_type()
            .is_socket()
    );
}

#[cfg(unix)]
#[test]
fn lf_review_tree_special_rejected_when_requested() {
    use std::os::unix::net::UnixListener;

    let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
    let root = std::fs::canonicalize(fixture.path()).expect("canonicalize fixture");
    let fs = crate::LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(&root, &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let source = root.join(stage.relative_name()).join("socket");
    drop(UnixListener::bind(&source).expect("socket"));
    let validation = LocalTreeValidationOptions::new().with_reject_special_files();
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("destination"),
            &LocalTreePublishOptions::new().with_validation(validation),
        )
        .expect_err("explicit special-file rule rejects socket");
    assert_eq!(
        error.cause().kind(),
        crate::error::LocalFileErrorKind::RequirementNotMet
    );
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert!(!root.join("destination").exists());
    error
        .into_stage()
        .expect("owner returned")
        .cleanup()
        .expect("stage cleanup");
}

#[cfg(unix)]
#[test]
fn lf_review_tree_hardlinks_reject_only_external_links() {
    for external in [false, true] {
        let fixture = tempfile::tempdir().expect("fixture");
        let fs = crate::LocalFileSystem::host().expect("Host");
        let parent = fs
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        let mut bytes = b"payload".as_slice();
        stage.write_file_new(Path::new("file"), &mut bytes, 7).expect("write");
        let root = fixture.path().join(stage.relative_name());
        if external {
            std::fs::hard_link(root.join("file"), fixture.path().join("outside")).expect("outside hardlink");
        } else {
            std::fs::hard_link(root.join("file"), root.join("alias")).expect("tree hardlink");
        }
        let validation = LocalTreeValidationOptions::new().with_reject_external_hardlinks();
        let result = stage.publish_new(
            &parent,
            OsStr::new("destination"),
            &LocalTreePublishOptions::new().with_validation(validation),
        );
        if external {
            let error = result.expect_err("outside link is rejected");
            assert_eq!(
                error.cause().kind(),
                crate::error::LocalFileErrorKind::RequirementNotMet
            );
            assert_eq!(error.publication(), LocalTreePublication::NotPublished);
            assert!(!fixture.path().join("destination").exists());
            error
                .into_stage()
                .expect("owner returned")
                .cleanup()
                .expect("stage cleanup");
        } else {
            let _ = result.expect("in-tree links are allowed");
            assert_eq!(
                std::fs::read(fixture.path().join("destination/file")).expect("published file"),
                b"payload"
            );
        }
    }
}

#[test]
fn lf_review_tree_path_budget_precedes_publish_retention() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = crate::LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let root = fixture.path().join(stage.relative_name());
    let mut current = root.clone();
    for _ in 0..18 {
        current.push("abcdef");
        std::fs::create_dir(&current).expect("nested directory");
    }
    std::fs::write(current.join("file"), b"x").expect("nested file");
    let validation =
        LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_seen_name_bytes(100));
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("destination"),
            &LocalTreePublishOptions::new().with_validation(validation),
        )
        .expect_err("cumulative logical path bytes exceed budget");
    assert_eq!(error.cause().kind(), crate::error::LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert!(!fixture.path().join("destination").exists());
    error
        .into_stage()
        .expect("owner retained")
        .cleanup()
        .expect("stage cleanup");
}

#[test]
fn lf_review_tree_publish_depth_limit_retains_stage() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = crate::LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let root = fixture.path().join(stage.relative_name());
    std::fs::create_dir(root.join("one")).expect("first level");
    std::fs::create_dir(root.join("one/two")).expect("second level");
    std::fs::write(root.join("one/two/file"), b"x").expect("deep file");
    crate::directory::reset_test_observation_count();
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("destination"),
            &LocalTreePublishOptions::new().with_validation(
                LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(1)),
            ),
        )
        .expect_err("observed over-depth entry rejects publication");
    assert_eq!(error.cause().kind(), crate::error::LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert!(!fixture.path().join("destination").exists());
    assert_eq!(
        crate::directory::test_observation_count(),
        2,
        "verification stops at the first over-depth entry"
    );
    error
        .into_stage()
        .expect("owner retained")
        .cleanup()
        .expect("stage cleanup");
}

#[test]
fn lf_review_tree_zero_deadline_and_entry_limit_retain_publish_owner() {
    for limits in [
        LocalListOptions::new().with_deadline(std::time::Duration::ZERO),
        LocalListOptions::new().with_max_entries(0),
    ] {
        let fixture = tempfile::tempdir().expect("fixture");
        let fs = crate::LocalFileSystem::host().expect("Host");
        let parent = fs
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        let mut bytes = b"x".as_slice();
        stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
        let error = stage
            .publish_new(
                &parent,
                OsStr::new("destination"),
                &LocalTreePublishOptions::new().with_validation(LocalTreeValidationOptions::new().with_limits(limits)),
            )
            .expect_err("zero validation budget rejects publication");
        assert_eq!(error.cause().kind(), crate::error::LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.publication(), LocalTreePublication::NotPublished);
        assert!(!fixture.path().join("destination").exists());
        error
            .into_stage()
            .expect("stage owner retained")
            .cleanup()
            .expect("stage cleanup");
    }
}

#[test]
fn lf3_tree_publish_conflict_retains_original() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir(fixture.path().join("target")).expect("target should be created");
    std::fs::write(fixture.path().join("target/old"), b"original").expect("target contents should be written");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let error = stage
        .publish_new(&parent, OsStr::new("target"), &LocalTreePublishOptions::new())
        .expect_err("existing target must not be replaced");
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert!(error.into_stage().is_some());
    assert_eq!(
        std::fs::read(fixture.path().join("target/old")).expect("target should remain readable"),
        b"original"
    );
}

#[test]
fn tree_publish_conflict_reports_prior_access_mutation() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir(fixture.path().join("target")).expect("target should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let name = stage.relative_name().to_os_string();
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("target"),
            &LocalTreePublishOptions::new().with_access(LocalTreeFinalAccess::CurrentUserOnly),
        )
        .expect_err("no-replace collision should fail");
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert_eq!(error.mutation_state(), crate::outcome::LocalTreeMutationState::Started);
    assert_eq!(error.access_state(), LocalTreeAccessState::CurrentUserOnly);
    let stage = error.into_stage().expect("unpublished stage remains owned");
    assert_eq!(stage.relative_name(), name.as_os_str());
    stage.cleanup().expect("stage remains cleanable");
    assert!(fixture.path().join("target").is_dir());
}

#[test]
fn test_recovery_error_tree_create_and_access_sources_retain_their_causes() {
    use crate::directory::LocalTreeAccessError;
    use crate::directory::LocalTreeCreateError;
    use crate::error::LocalFileError;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileOperation;
    use crate::outcome::LocalTreeAccessState;
    use crate::outcome::LocalTreeMutationState;

    let create_cause = LocalFileError::new(LocalFileErrorKind::NotFound, LocalFileOperation::CreateDirectory);
    let create_error = LocalTreeCreateError::new(create_cause, None, Vec::new(), LocalTreeMutationState::NotStarted);
    let create_source = std::error::Error::source(&create_error)
        .and_then(|source| source.downcast_ref::<LocalFileError>())
        .expect("create error should expose its structured cause");
    assert!(std::ptr::eq(create_source, create_error.cause()));

    let access_cause = LocalFileError::new(LocalFileErrorKind::PermissionDenied, LocalFileOperation::Commit);
    let access_error = LocalTreeAccessError::new(access_cause, LocalTreeAccessState::Preserved);
    let access_source = std::error::Error::source(&access_error)
        .and_then(|source| source.downcast_ref::<LocalFileError>())
        .expect("access error should expose its structured cause");
    assert!(std::ptr::eq(access_source, access_error.cause()));
}

#[test]
fn test_recovery_error_tree_cleanup_source_retains_its_cause_and_owner() {
    use crate::options::LocalTempCleanupLimits;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = crate::LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0));
    let error = stage.cleanup().expect_err("zero entry budget should prevent cleanup");
    let source = std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<crate::LocalFileError>())
        .expect("cleanup error should expose its structured cause");
    assert!(std::ptr::eq(source, error.cause()));
    for mut resource in error.into_retained_resources() {
        resource.set_cleanup_limits(LocalTempCleanupLimits::new());
        resource.cleanup().expect("retained owner should remain cleanable");
    }
}

#[cfg(unix)]
#[test]
fn tree_access_policy_round_trips_without_releasing_stage() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut input = &b"before"[..];
    stage
        .write_file_new(std::path::Path::new("file"), &mut input, 16)
        .expect("file should be staged");
    let before = stage.identity().cloned();
    let original_mode = std::fs::metadata(fixture.path().join(stage.relative_name()))
        .expect("stage permissions should be observed")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        stage
            .apply_access_policy(LocalTreeFinalAccess::Preserve)
            .expect("preserve access policy should not mutate the stage")
            .access_state(),
        LocalTreeAccessState::Preserved
    );
    assert_eq!(
        std::fs::metadata(fixture.path().join(stage.relative_name()))
            .expect("stage permissions should remain observable")
            .permissions()
            .mode()
            & 0o777,
        original_mode
    );
    assert_eq!(
        stage
            .apply_access_policy(LocalTreeFinalAccess::ReadOnlyCurrentUser)
            .expect("read-only policy should apply")
            .access_state(),
        LocalTreeAccessState::ReadOnlyCurrentUser
    );
    assert_eq!(
        std::fs::metadata(fixture.path().join(stage.relative_name()).join("file"))
            .expect("file should remain")
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
    assert_eq!(
        stage
            .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
            .expect("writable policy should apply")
            .access_state(),
        LocalTreeAccessState::CurrentUserOnly
    );
    let mut input = &b"after"[..];
    stage
        .write_file_new(std::path::Path::new("again"), &mut input, 16)
        .expect("stage should be writable again");
    assert_eq!(stage.identity(), before.as_ref());
}

#[test]
fn tree_publish_rejects_non_component_names_without_consuming_stage() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let error = stage
        .publish_new(&parent, OsStr::new("a/b"), &LocalTreePublishOptions::new())
        .expect_err("separator must be rejected");
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert!(error.into_stage().is_some());
}

#[test]
fn retained_stage_can_be_reopened_by_identity_and_explicitly_cleaned() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let relative = std::path::PathBuf::from(stage.relative_name());
    let identity = stage.identity().cloned().expect("stage identity should be known");
    let location = stage.retain_in_place();
    assert_eq!(location.identity(), Some(&identity));
    drop(location);
    let observed = parent.observe_entry(&relative).expect("retained stage should remain");
    assert_eq!(observed.identity(), Some(&identity));
    let reopened = parent
        .open_staged_directory(&relative, &identity, &crate::options::LocalTreeValidationOptions::new())
        .expect("stage should reopen after identity validation");
    reopened.cleanup().expect("matching stage should be explicitly cleaned");
    assert!(
        parent
            .entry_metadata(&relative)
            .expect("entry lookup should succeed")
            .is_none()
    );
}

#[test]
fn successful_publish_keeps_stage_contents_at_target() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut input = &b"payload"[..];
    stage
        .write_file_new(std::path::Path::new("file"), &mut input, 16)
        .expect("file should be staged");
    let outcome = stage
        .publish_new(&parent, OsStr::new("published"), &LocalTreePublishOptions::new())
        .expect("stage should publish");
    assert_eq!(outcome.publication(), LocalTreePublication::Published);
    drop(outcome);
    assert_eq!(
        std::fs::read(fixture.path().join("published/file")).expect("published file should remain"),
        b"payload"
    );
}

#[test]
fn required_file_sync_is_completed_before_publish() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut input = &b"payload"[..];
    stage
        .write_file_new(std::path::Path::new("file"), &mut input, 16)
        .expect("file should be staged");
    let options = LocalTreePublishOptions::new().with_file_sync(crate::policy::LocalFileSyncRequirement::Required);
    let outcome = stage
        .publish_new(&parent, OsStr::new("published"), &options)
        .expect("required file sync should succeed");
    assert!(outcome.file_synced());
    assert_eq!(outcome.publication(), LocalTreePublication::Published);
}

#[cfg(unix)]
#[test]
fn readonly_publish_collision_restores_stage_access() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(fixture.path().join("published"), b"existing").expect("destination");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser),
        )
        .expect_err("existing destination must be preserved");
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert_eq!(error.access_state(), LocalTreeAccessState::ReadOnlyCurrentUser);
    let mut stage = error.into_stage().expect("unpublished stage should be retained");
    assert_eq!(
        stage
            .observation()
            .expect("stage observation")
            .metadata()
            .permissions()
            .unix_mode(),
        Some(0o500)
    );
    stage
        .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
        .expect("restore stage access");
    stage.cleanup().expect("cleanup");
    assert_eq!(
        std::fs::read(fixture.path().join("published")).expect("destination bytes"),
        b"existing"
    );
}

#[cfg(unix)]
#[test]
fn readonly_publish_collision_and_restore_failure_preserve_both_errors() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(fixture.path().join("published"), b"existing").expect("destination");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("file"), &mut &b"payload"[..], 16)
        .expect("file should be staged");
    crate::temp::internal::privacy_unix::fail_access_after(3);
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser),
        )
        .expect_err("collision and access failure must be reported");
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert_eq!(error.access_state(), LocalTreeAccessState::Indeterminate);
    assert!(error.cause().cleanup_error().is_some());
    let mut stage = error.into_stage().expect("unpublished stage should be retained");
    stage
        .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
        .expect("restore stage access");
    stage.cleanup().expect("cleanup");
    assert_eq!(
        std::fs::read(fixture.path().join("published")).expect("destination bytes"),
        b"existing"
    );
}

#[cfg(unix)]
#[test]
fn readonly_publish_final_access_failure_reports_published() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("file"), &mut &b"payload"[..], 16)
        .expect("file should be staged");
    crate::temp::internal::privacy_unix::fail_access_after(3);
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser),
        )
        .expect_err("final root permission failure must be reported");
    assert_eq!(error.publication(), LocalTreePublication::Published);
    assert_eq!(error.mutation_state(), crate::outcome::LocalTreeMutationState::Started);
    assert_eq!(error.access_state(), LocalTreeAccessState::Indeterminate);
    assert!(error.into_stage().is_none());
    assert_eq!(
        std::fs::read(fixture.path().join("published/file")).expect("published payload"),
        b"payload"
    );
}

#[cfg(unix)]
#[test]
fn lf_revision_readonly_sync_publishes_all_sync_requirements() {
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "this evidence must come from a non-root Unix user"
    );
    for requirement in [
        crate::policy::LocalFileSyncRequirement::NotRequired,
        crate::policy::LocalFileSyncRequirement::Preferred,
        crate::policy::LocalFileSyncRequirement::Required,
    ] {
        let fixture = tempfile::tempdir().expect("fixture should be created");
        let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
        let parent = fs
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent should open");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage should be created");
        let mut input = &b"payload"[..];
        stage
            .write_file_new(Path::new("file"), &mut input, 16)
            .expect("file should be staged");
        let options = LocalTreePublishOptions::new()
            .with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser)
            .with_file_sync(requirement);
        let outcome = stage
            .publish_new(&parent, OsStr::new("published"), &options)
            .expect("read-only stage should publish with requested file sync");
        assert_eq!(outcome.publication(), LocalTreePublication::Published);
        assert_eq!(
            std::os::unix::fs::MetadataExt::mode(
                &std::fs::metadata(fixture.path().join("published")).expect("root metadata")
            ) & 0o777,
            0o500,
        );
        assert_eq!(
            outcome.file_synced(),
            requirement != crate::policy::LocalFileSyncRequirement::NotRequired
        );
        let path = fixture.path().join("published/file");
        assert_eq!(
            std::fs::read(&path).expect("published payload should be readable"),
            b"payload"
        );
        assert_eq!(
            std::os::unix::fs::MetadataExt::mode(&std::fs::metadata(path).expect("published metadata")) & 0o777,
            0o400,
        );
    }
}

#[cfg(unix)]
#[test]
fn lf_revision_readonly_sync_failure_states_preserve_truthful_outcomes() {
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "this evidence must come from a non-root Unix user"
    );
    for requirement in [
        crate::policy::LocalFileSyncRequirement::Preferred,
        crate::policy::LocalFileSyncRequirement::Required,
    ] {
        let fixture = tempfile::tempdir().expect("fixture should be created");
        let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
        let parent = fs
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent should open");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage should be created");
        let mut input = &b"payload"[..];
        stage
            .write_file_new(Path::new("file"), &mut input, 16)
            .expect("file should be staged");
        crate::directory::fail_tree_sync_after(0);
        let options = LocalTreePublishOptions::new()
            .with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser)
            .with_file_sync(requirement);
        match stage.publish_new(&parent, OsStr::new("published"), &options) {
            Ok(outcome) => {
                assert_eq!(requirement, crate::policy::LocalFileSyncRequirement::Preferred);
                assert_eq!(outcome.publication(), LocalTreePublication::Published);
                assert!(
                    !outcome.file_synced(),
                    "failed preferred sync must not be reported as complete"
                );
                assert_eq!(
                    std::fs::read(fixture.path().join("published/file")).expect("published payload"),
                    b"payload"
                );
            }
            Err(error) => {
                assert_eq!(requirement, crate::policy::LocalFileSyncRequirement::Required);
                assert_eq!(error.publication(), LocalTreePublication::NotPublished);
                assert_eq!(error.mutation_state(), crate::outcome::LocalTreeMutationState::Started);
                assert_eq!(error.access_state(), LocalTreeAccessState::Mixed);
                assert!(
                    parent
                        .entry_metadata(Path::new("published"))
                        .expect("destination lookup should succeed")
                        .is_none()
                );
                let mut retained = error.into_stage().expect("failed publish retains stage");
                retained
                    .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
                    .expect("restore writable stage permissions before cleanup");
                retained.cleanup().expect("restored stage should be cleanable");
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn lf_revision_preserve_sync_uses_a_write_capable_handle() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut input = &b"payload"[..];
    stage
        .write_file_new(Path::new("file"), &mut input, 16)
        .expect("file should be staged");

    let outcome = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_access(LocalTreeFinalAccess::Preserve)
                .with_file_sync(crate::policy::LocalFileSyncRequirement::Required),
        )
        .expect("preserved stage should synchronize and publish");

    assert_eq!(outcome.publication(), LocalTreePublication::Published);
    assert!(outcome.file_synced());
    assert_eq!(
        std::fs::read(fixture.path().join("published/file")).expect("published payload"),
        b"payload"
    );
}

#[cfg(windows)]
#[test]
fn windows_access_failure_after_child_mutation_reports_mixed_and_keeps_stage() {
    use crate::temp::internal::privacy_windows;
    use crate::temp::internal::privacy_windows::FailurePoint;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut input = &b"payload"[..];
    stage
        .write_file_new(std::path::Path::new("file"), &mut input, 16)
        .expect("file should be staged");
    privacy_windows::fail_after(FailurePoint::AccessApplyBefore, 1);
    let error = stage
        .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
        .expect_err("injected root ACL failure should be reported");
    assert_eq!(error.access_state(), LocalTreeAccessState::Mixed);
    assert!(error.retained_resources().is_empty());
    assert!(stage.observation().is_ok(), "the caller must retain the stage owner");
    drop(error);
    assert_eq!(
        stage
            .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
            .expect("retry should apply all ACLs")
            .access_state(),
        LocalTreeAccessState::CurrentUserOnly
    );
}

#[cfg(unix)]
#[test]
fn unix_access_failure_after_child_mutation_reports_mixed_and_keeps_stage() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut input = &b"payload"[..];
    stage
        .write_file_new(std::path::Path::new("file"), &mut input, 16)
        .expect("file should be staged");
    crate::temp::internal::privacy_unix::fail_access_after(1);
    let error = stage
        .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
        .expect_err("injected root permission failure should be reported");
    assert_eq!(error.access_state(), LocalTreeAccessState::Mixed);
    assert!(error.retained_resources().is_empty());
    assert!(stage.observation().is_ok(), "the caller must retain the stage owner");
    drop(error);
    assert_eq!(
        stage
            .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
            .expect("retry should apply all permissions")
            .access_state(),
        LocalTreeAccessState::CurrentUserOnly
    );
}

#[test]
fn required_sync_failure_keeps_unpublished_stage_for_cleanup() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let fs = crate::LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut input = &b"payload"[..];
    stage
        .write_file_new(std::path::Path::new("file"), &mut input, 16)
        .expect("file should be staged");
    crate::directory::fail_tree_sync_after(0);
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_file_sync(crate::policy::LocalFileSyncRequirement::Required),
        )
        .expect_err("injected required sync failure must prevent publication");
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert_eq!(
        error.mutation_state(),
        crate::outcome::LocalTreeMutationState::NotStarted
    );
    assert!(
        parent
            .entry_metadata(std::path::Path::new("published"))
            .expect("target lookup should succeed")
            .is_none()
    );
    error
        .into_stage()
        .expect("unpublished stage should remain available")
        .cleanup()
        .expect("stage should be cleanable");
}
