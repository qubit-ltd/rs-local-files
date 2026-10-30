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
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
#[cfg(target_os = "linux")]
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalPrivacyRequirement;
use tempfile::TempDir;

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
