// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::policy::LocalFileSyncRequirement;
#[cfg(target_os = "linux")]
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalPrivacyRequirement;
use tempfile::TempDir;

#[test]
fn test_staged_directory_exposes_bounded_file_synchronization() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("entry"), &mut &b"payload"[..], 16)
        .expect("file should be written");

    assert!(
        stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::Required)
            .expect("required file synchronization should succeed")
    );
    assert!(
        !stage
            .sync_files_with_options(&LocalListOptions::new(), LocalFileSyncRequirement::NotRequired)
            .expect("not-required synchronization should succeed")
    );
    #[cfg(unix)]
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("the staged tree, root, and parent should synchronize");
    #[cfg(windows)]
    assert!(matches!(
        stage
            .sync_tree_with_options(&parent, &LocalListOptions::new())
            .expect_err("Windows directory synchronization may be denied")
            .kind(),
        LocalFileErrorKind::Unsupported | LocalFileErrorKind::PermissionDenied
    ));
}

#[cfg(unix)]
#[test]
fn test_staged_tree_syncs_nested_directories_before_its_root_and_parent() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    #[cfg(target_os = "linux")]
    let options = LocalTempDirectoryOptions::new()
        .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600));
    #[cfg(not(target_os = "linux"))]
    let options = LocalTempDirectoryOptions::new();
    let mut stage = parent
        .create_staged_directory(&options)
        .expect("stage should be created");
    let _ = stage
        .directory()
        .create_directory_with_options(Path::new("nested"), &LocalCreateDirectoryOptions::new())
        .expect("nested stage directory should be created");
    stage
        .write_file_new(Path::new("nested/entry"), &mut &b"payload"[..], 16)
        .expect("nested file should be written");

    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("nested files, directories, stage root, and parent should sync");
}

fn stage_parent(scratch: &TempDir) -> LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
        .expect("stage parent should open")
}

fn assert_replaced_stage_entry_is_preserved(relative: &Path, options: LocalTempDirectoryOptions) {
    let scratch = tempfile::tempdir().expect("scratch directory should exist");
    let parent = stage_parent(&scratch);
    let mut stage = parent
        .create_staged_directory(&options)
        .expect("stage should be created");
    let stage_path = scratch.path().join(stage.relative_name());
    if relative == Path::new("nested/../file") {
        let nested = stage_path.join("nested");
        fs::create_dir(&nested).expect("nested directory should exist first");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&nested, fs::Permissions::from_mode(0o700))
                .expect("nested directory should satisfy owner policy");
        }
    }

    stage
        .write_file_new(relative, &mut &b"original"[..], 8)
        .expect("original stage entry should be created");
    let saved_original_path = scratch.path().join("saved-original");
    fs::rename(stage_path.join("file"), &saved_original_path)
        .expect("original stage entry should be moved outside the stage");
    let write_error = stage
        .write_file_new(relative, &mut &b"replacement"[..], 11)
        .expect_err("the retained index must reject replacing its recorded identity");
    assert_eq!(LocalFileErrorKind::InvalidState, write_error.kind());

    let replacement_path = stage_path.join("file");
    assert_eq!(
        b"replacement".as_slice(),
        fs::read(&replacement_path)
            .expect("replacement should exist after the write error")
            .as_slice()
    );
    let error = stage.cleanup().expect_err("cleanup must reject a replaced stage entry");
    assert_eq!(LocalFileErrorKind::InvalidState, error.cause().kind());
    assert_eq!(
        b"replacement".as_slice(),
        fs::read(&replacement_path)
            .expect("replacement should survive")
            .as_slice()
    );
    assert_eq!(
        b"original".as_slice(),
        fs::read(&saved_original_path)
            .expect("original should survive")
            .as_slice()
    );
}

#[test]
fn test_stage_cleanup_checks_owned_identity_for_path_aliases() {
    for relative in [Path::new("file"), Path::new("./file"), Path::new("nested/../file")] {
        assert_replaced_stage_entry_is_preserved(relative, LocalTempDirectoryOptions::new());
    }
}

#[cfg(any(unix, windows))]
#[test]
fn test_private_stage_cleanup_checks_owned_identity_for_path_aliases() {
    assert_replaced_stage_entry_is_preserved(
        Path::new("nested/../file"),
        LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_owner_policy_stage_cleanup_checks_owned_identity_for_path_aliases() {
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    assert_replaced_stage_entry_is_preserved(
        Path::new("nested/../file"),
        LocalTempDirectoryOptions::new().with_owner_access_policy(policy),
    );
}

#[test]
fn test_stage_cleanup_succeeds_for_unreplaced_path_aliases() {
    for relative in [Path::new("file"), Path::new("./file"), Path::new("nested/../file")] {
        let scratch = tempfile::tempdir().expect("scratch directory should exist");
        let parent = stage_parent(&scratch);
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage should be created");
        let stage_path = scratch.path().join(stage.relative_name());
        if relative == Path::new("nested/../file") {
            let nested = stage_path.join("nested");
            fs::create_dir(&nested).expect("nested directory should exist first");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&nested, fs::Permissions::from_mode(0o700))
                    .expect("nested directory should be safely removable");
            }
        }
        stage
            .write_file_new(relative, &mut &b"original"[..], 8)
            .expect("stage entry should be created");

        stage.cleanup().expect("unreplaced stage should clean up");
        assert!(!stage_path.exists(), "stage directory should be removed");
    }
}
