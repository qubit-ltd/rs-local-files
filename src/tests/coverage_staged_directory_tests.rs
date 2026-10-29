use std::path::Path;

use crate::LocalFileSystem;
use crate::LocalListOptions;
use crate::directory::LocalTreeEntry;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTreeValidationOptions;
use crate::outcome::LocalFileKind;
use crate::policy::LocalTreeFinalAccess;

#[test]
fn staged_directory_observes_validates_and_applies_access_policy() {
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
        .collect::<crate::LocalResult<Vec<_>>>()
        .expect("tree entries should be observed");
    assert_eq!(observed.len(), 2);

    let result = stage
        .apply_access_policy_with_options(LocalTreeFinalAccess::CurrentUserOnly, &LocalListOptions::new())
        .expect("owner-only access should apply");
    assert_eq!(
        result.access_state(),
        crate::outcome::LocalTreeAccessState::CurrentUserOnly
    );
    stage.cleanup().expect("stage should clean up");
}

#[test]
#[cfg(unix)]
fn staged_owner_policy_limits_fail_before_mutation_and_keep_the_stage() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("one"), &mut &b"1"[..], 1)
        .expect("first file should be written");
    stage
        .write_file_new(Path::new("two"), &mut &b"2"[..], 1)
        .expect("second file should be written");

    let policy = crate::policy::LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new().with_max_entries(1))
        .expect_err("entry ceiling should reject the larger tree");
    assert_eq!(error.cause().kind(), crate::LocalFileErrorKind::ResourceLimit);
    stage.cleanup().expect("stage should clean up");
}

#[cfg(target_os = "linux")]
#[test]
fn staged_owner_policy_applies_to_nested_files_and_directories() {
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

    let policy = crate::policy::LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let outcome = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())
        .expect("owner policy should be applied and verified");
    assert_eq!(
        outcome.access_state(),
        crate::outcome::LocalTreeAccessState::OwnerPolicyApplied
    );
    stage.cleanup().expect("stage should clean up");
}

#[cfg(unix)]
#[test]
fn private_staged_directory_writes_files_through_its_retained_parent_authority() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new().with_privacy(crate::policy::LocalPrivacyRequirement::Required),
        )
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
