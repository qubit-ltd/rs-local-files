// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalResult;
use qubit_local_files::directory::LocalTreeEntry;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreeValidationOptions;
use qubit_local_files::outcome::LocalFileKind;
use qubit_local_files::outcome::LocalTreeAccessState;
#[cfg(target_os = "linux")]
use qubit_local_files::policy::LocalOwnerAccessPolicy;
#[cfg(unix)]
use qubit_local_files::policy::LocalPrivacyRequirement;
use qubit_local_files::policy::LocalTreeFinalAccess;

#[test]
fn test_staged_directory_observes_validates_and_applies_access_policy() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    std::fs::create_dir(fixture.path().join(stage.relative_name()).join("nested"))
        .expect("nested directory should be created");
    stage
        .write_file_new(Path::new("nested/file"), &mut &b"value"[..], 16)
        .expect("staged file should be written");

    assert_eq!(
        stage.observation().expect("stage should be observed").metadata().kind(),
        LocalFileKind::Directory
    );
    let entries = [
        LocalTreeEntry::new(Path::new("nested").to_path_buf(), LocalFileKind::Directory)
            .expect("directory entry should be valid"),
        LocalTreeEntry::new(Path::new("nested/file").to_path_buf(), LocalFileKind::File)
            .expect("file entry should be valid"),
    ];
    stage
        .verify_entries(&entries, &LocalTreeValidationOptions::new())
        .expect("expected entries should match the staged tree");
    let observed = stage
        .observe_tree_with_options(&LocalListOptions::new())
        .expect("tree observer should be created")
        .collect::<LocalResult<Vec<_>>>()
        .expect("tree entries should be observed");
    assert_eq!(observed.len(), 2);

    let result = stage
        .apply_access_policy_with_options(LocalTreeFinalAccess::CurrentUserOnly, &LocalListOptions::new())
        .expect("owner-only access should apply");
    assert_eq!(result.access_state(), LocalTreeAccessState::CurrentUserOnly);
    stage.cleanup().expect("stage should clean up");
}

#[cfg(target_os = "linux")]
#[test]
fn test_staged_owner_policy_applies_to_nested_files_and_directories() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    std::fs::create_dir(fixture.path().join(stage.relative_name()).join("nested"))
        .expect("nested directory should be created");
    stage
        .write_file_new(Path::new("nested/file"), &mut &b"payload"[..], 16)
        .expect("nested file should be written");

    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let outcome = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())
        .expect("owner policy should be applied and verified");
    assert_eq!(
        outcome.access_state(),
        qubit_local_files::outcome::LocalTreeAccessState::OwnerPolicyApplied
    );
    stage.cleanup().expect("stage should clean up");
}

#[cfg(unix)]
#[test]
fn test_staged_owner_policy_requires_traversal_only_for_nonempty_stages() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o300, 0o000);

    let mut nonempty = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("nonempty stage should be created");
    nonempty
        .write_file_new(Path::new("payload"), &mut &b"value"[..], 5)
        .expect("payload should be written");
    let nonempty_path = fixture.path().join(nonempty.relative_name());
    let error = nonempty
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())
        .expect_err("nonempty stage must remain traversable for verification");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::RequirementNotMet,
        error.cause().kind()
    );
    assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
    assert_eq!(
        0o700,
        std::fs::metadata(&nonempty_path)
            .expect("stage metadata")
            .permissions()
            .mode()
            & 0o777
    );
    assert_eq!(
        b"value",
        std::fs::read(nonempty_path.join("payload"))
            .expect("payload remains readable")
            .as_slice()
    );
    nonempty.cleanup().expect("rejected stage should clean up");

    let mut empty = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("empty stage should be created");
    let empty_path = fixture.path().join(empty.relative_name());
    let outcome = empty
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())
        .expect("an empty stage can use non-traversable permissions");
    assert_eq!(
        qubit_local_files::outcome::LocalTreeAccessState::OwnerPolicyApplied,
        outcome.access_state()
    );
    assert_eq!(
        0o300,
        std::fs::metadata(&empty_path)
            .expect("empty stage metadata")
            .permissions()
            .mode()
            & 0o777
    );
    std::fs::set_permissions(&empty_path, std::fs::Permissions::from_mode(0o700)).expect("restore cleanup traversal");
    empty.cleanup().expect("empty stage should clean up");
}

#[cfg(unix)]
#[test]
fn test_private_staged_directory_writes_files_through_its_retained_parent_authority() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect("private stage should be created");
    let observation = stage
        .write_file_new(Path::new("private"), &mut &b"secret"[..], 16)
        .expect("private file should be written");
    assert_eq!(observation.metadata().len(), 6);
    assert_eq!(
        std::fs::read(fixture.path().join(stage.relative_name()).join("private"))
            .expect("private file should be readable by its owner"),
        b"secret"
    );
    stage.cleanup().expect("private stage should clean up");
}
