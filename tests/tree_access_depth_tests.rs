// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalStagedDirectory;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::policy::LocalTreeFinalAccess;
use tempfile::TempDir;

fn staged_tree(
    fixture: &TempDir,
    nested: bool,
) -> (
    qubit_local_files::directory::LocalDirectoryHandle,
    LocalStagedDirectory,
    std::path::PathBuf,
) {
    let fs = LocalFileSystem::host().expect("host filesystem");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let root = fixture.path().join(stage.relative_name());
    let leaf = if nested {
        std::fs::create_dir(root.join("one")).expect("first directory");
        Path::new("one/leaf")
    } else {
        Path::new("leaf")
    };
    stage.write_file_new(leaf, &mut &b"payload"[..], 16).expect("leaf file");
    (parent, stage, root)
}

#[test]
fn lf_n02_empty_tree_at_zero_depth_is_allowed() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("host filesystem");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");

    stage
        .apply_access_policy_with_options(
            LocalTreeFinalAccess::CurrentUserOnly,
            &LocalListOptions::new().with_max_depth(0),
        )
        .expect("empty tree requires no child traversal");
    stage
        .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
        .expect("restore writable root permissions");
    stage.cleanup().expect("cleanup");
}

#[test]
fn lf_n02_nonempty_tree_zero_depth_fails_without_partial_permission_changes() {
    #[cfg(unix)]
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "permission evidence requires a non-root user"
    );

    for policy in [
        LocalTreeFinalAccess::CurrentUserOnly,
        LocalTreeFinalAccess::ReadOnlyCurrentUser,
    ] {
        let fixture = tempfile::tempdir().expect("fixture");
        let (_parent, mut stage, root) = staged_tree(&fixture, false);
        let leaf = root.join("leaf");
        #[cfg(unix)]
        let original_modes = {
            use std::os::unix::fs::MetadataExt;
            (
                std::fs::metadata(&root).expect("root metadata").mode() & 0o777,
                std::fs::metadata(&leaf).expect("leaf metadata").mode() & 0o777,
            )
        };

        let failure = stage
            .apply_access_policy_with_options(policy, &LocalListOptions::new().with_max_depth(0))
            .expect_err("non-empty tree exceeds zero depth");
        assert_eq!(failure.cause().kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(
            failure.access_state(),
            qubit_local_files::outcome::LocalTreeAccessState::Indeterminate
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                original_modes.0,
                std::fs::metadata(&root).expect("root metadata").mode() & 0o777
            );
            assert_eq!(
                original_modes.1,
                std::fs::metadata(&leaf).expect("leaf metadata").mode() & 0o777
            );
        }
        stage
            .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
            .expect("restore writable permissions");
        stage.cleanup().expect("cleanup");
    }
}

#[test]
fn lf_n02_depth_boundary_is_inclusive_for_both_access_policies() {
    for (nested, maximum) in [(false, 1), (true, 2)] {
        for policy in [
            LocalTreeFinalAccess::CurrentUserOnly,
            LocalTreeFinalAccess::ReadOnlyCurrentUser,
        ] {
            let fixture = tempfile::tempdir().expect("fixture");
            let (_parent, mut stage, root) = staged_tree(&fixture, nested);
            stage
                .apply_access_policy_with_options(policy, &LocalListOptions::new().with_max_depth(maximum))
                .expect("entry at the configured depth is valid");
            if cfg!(unix) && policy == LocalTreeFinalAccess::ReadOnlyCurrentUser {
                stage
                    .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
                    .expect("restore writable permissions");
            }
            if cfg!(windows) && policy == LocalTreeFinalAccess::ReadOnlyCurrentUser {
                let leaf = root.join(if nested { "one/leaf" } else { "leaf" });
                let mut permissions = std::fs::metadata(&leaf)
                    .expect("read-only leaf metadata should be available")
                    .permissions();
                permissions.set_readonly(false);
                std::fs::set_permissions(&leaf, permissions).expect("clear read-only attribute before cleanup");
            }
            stage.cleanup().expect("cleanup");
        }
    }
}

#[test]
fn lf_n02_over_depth_nested_tree_fails_before_any_permission_change() {
    for policy in [
        LocalTreeFinalAccess::CurrentUserOnly,
        LocalTreeFinalAccess::ReadOnlyCurrentUser,
    ] {
        let fixture = tempfile::tempdir().expect("fixture");
        let (_parent, mut stage, root) = staged_tree(&fixture, true);
        let leaf = root.join("one/leaf");
        #[cfg(unix)]
        let original_modes = {
            use std::os::unix::fs::MetadataExt;
            [
                std::fs::metadata(&root).expect("root metadata").mode() & 0o777,
                std::fs::metadata(root.join("one")).expect("directory metadata").mode() & 0o777,
                std::fs::metadata(&leaf).expect("leaf metadata").mode() & 0o777,
            ]
        };
        let failure = stage
            .apply_access_policy_with_options(policy, &LocalListOptions::new().with_max_depth(1))
            .expect_err("nested tree exceeds the depth ceiling");
        assert_eq!(failure.cause().kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(
            failure.access_state(),
            qubit_local_files::outcome::LocalTreeAccessState::Indeterminate
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                original_modes[0],
                std::fs::metadata(&root).expect("root metadata").mode() & 0o777
            );
            assert_eq!(
                original_modes[1],
                std::fs::metadata(root.join("one")).expect("directory metadata").mode() & 0o777
            );
            assert_eq!(
                original_modes[2],
                std::fs::metadata(&leaf).expect("leaf metadata").mode() & 0o777
            );
        }
        stage
            .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
            .expect("restore writable permissions");
        stage.cleanup().expect("cleanup");
    }
}
