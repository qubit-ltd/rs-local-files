use std::ffi::OsStr;

use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTreePublishOptions;
use crate::outcome::LocalTreeAccessState;
use crate::outcome::LocalTreePublication;
use crate::policy::LocalTreeFinalAccess;

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

#[cfg(windows)]
#[test]
fn windows_access_failure_after_child_mutation_reports_mixed_and_keeps_stage() {
    use crate::temp::internal::privacy_windows::FailurePoint;
    use crate::temp::internal::privacy_windows::{self};

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
