// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalStagedDirectory;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::outcome::LocalTreeAccessState;
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalTreeFinalAccess;

fn fixture(with_file: bool) -> (tempfile::TempDir, LocalStagedDirectory, LocalOwnerAccessPolicy) {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    if with_file {
        stage
            .write_file_new(Path::new("payload"), &mut &b"original"[..], 8)
            .expect("original tracked file");
    }
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    (scratch, stage, policy)
}

#[test]
fn test_remaining_owner_policy_validation_preserves_existing_state() {
    let (scratch, mut stage, valid) = fixture(true);
    let native = scratch.path().join(stage.relative_name());
    stage
        .apply_owner_access_policy_with_options(&valid, &LocalListOptions::new())
        .expect("establish valid owner policy");
    for invalid in [
        LocalOwnerAccessPolicy::new(),
        LocalOwnerAccessPolicy::new().with_windows_masks(0x001f_01ff, 0x001f_01ff),
        LocalOwnerAccessPolicy::new().with_unix_modes(0o755, 0o600),
        LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o644),
    ] {
        let error = stage
            .apply_owner_access_policy_with_options(&invalid, &LocalListOptions::new())
            .expect_err("invalid platform owner policy must fail before touching stage state");
        assert_eq!(LocalFileErrorKind::InvalidOptions, error.cause().kind());
        assert_eq!(LocalFileOperation::ValidateName, error.cause().operation());
        assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
        assert_eq!(Some(&valid), stage.owner_access_policy());
        assert_eq!(
            0o600,
            fs::metadata(native.join("payload"))
                .expect("preserved file metadata")
                .permissions()
                .mode()
                & 0o777
        );
        assert_eq!(
            0o700,
            fs::metadata(&native)
                .expect("preserved root metadata")
                .permissions()
                .mode()
                & 0o777
        );
        assert_eq!(
            b"original",
            fs::read(native.join("payload")).expect("preserved data").as_slice()
        );
    }
    stage
        .apply_owner_access_policy_with_options(&valid, &LocalListOptions::new())
        .expect("valid policy still succeeds");
    stage.cleanup().expect("cleanup valid retained state");
    assert!(!native.exists());
}

#[test]
fn test_remaining_access_name_byte_budget_exact_boundary_and_retry() {
    for owner in [false, true] {
        let (scratch, mut stage, policy) = fixture(true);
        let native = scratch.path().join(stage.relative_name());
        fs::set_permissions(native.join("payload"), fs::Permissions::from_mode(0o644)).expect("initial file mode");
        fs::set_permissions(&native, fs::Permissions::from_mode(0o755)).expect("initial root mode");
        let limits = LocalListOptions::new().with_max_seen_name_bytes(6);
        let result = if owner {
            stage.apply_owner_access_policy_with_options(&policy, &limits)
        } else {
            stage.apply_access_policy_with_options(LocalTreeFinalAccess::CurrentUserOnly, &limits)
        };
        let error = result.expect_err("seven-byte payload name exceeds six-byte budget");
        assert_eq!(LocalFileErrorKind::ResourceLimit, error.cause().kind());
        assert_eq!(
            if owner {
                LocalTreeAccessState::Preserved
            } else {
                LocalTreeAccessState::Indeterminate
            },
            error.access_state()
        );
        assert_eq!(
            0o644,
            fs::metadata(native.join("payload"))
                .expect("pre-mutation file mode")
                .permissions()
                .mode()
                & 0o777
        );
        assert_eq!(
            0o755,
            fs::metadata(&native)
                .expect("pre-mutation root mode")
                .permissions()
                .mode()
                & 0o777
        );
        for budget in [7, 8] {
            let limits = LocalListOptions::new().with_max_seen_name_bytes(budget);
            let result = if owner {
                stage.apply_owner_access_policy_with_options(&policy, &limits)
            } else {
                stage.apply_access_policy_with_options(LocalTreeFinalAccess::CurrentUserOnly, &limits)
            }
            .expect("inclusive budget permits full collection and final verification");
            assert_eq!(
                if owner {
                    LocalTreeAccessState::OwnerPolicyApplied
                } else {
                    LocalTreeAccessState::CurrentUserOnly
                },
                result.access_state()
            );
            assert_eq!(
                b"original",
                fs::read(native.join("payload")).expect("data preserved").as_slice()
            );
            assert_eq!(
                0o600,
                fs::metadata(native.join("payload"))
                    .expect("final file mode")
                    .permissions()
                    .mode()
                    & 0o777
            );
            assert_eq!(
                0o700,
                fs::metadata(&native).expect("final root mode").permissions().mode() & 0o777
            );
        }
        stage.cleanup().expect("cleanup after budget retry");
        assert!(!native.exists());
    }
}
