// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests ordinary staged access failures through crate test fault injection.

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::LocalFileSystem;
use crate::directory::LocalStagedDirectory;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::outcome::LocalTreeAccessState;
use crate::policy::LocalOwnerAccessPolicy;
use crate::policy::LocalTreeFinalAccess;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

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
fn test_remaining_ordinary_access_root_clone_and_final_observation_failures() {
    for policy in [
        LocalTreeFinalAccess::CurrentUserOnly,
        LocalTreeFinalAccess::ReadOnlyCurrentUser,
    ] {
        for (point, nth) in [(NativeIoPoint::CloneAuthority, 2), (NativeIoPoint::Observation, 3)] {
            let (scratch, mut stage, _) = fixture(true);
            let native = scratch.path().join(stage.relative_name());
            fs::set_permissions(native.join("payload"), fs::Permissions::from_mode(0o644)).expect("initial file mode");
            fs::set_permissions(&native, fs::Permissions::from_mode(0o755)).expect("initial root mode");
            let fault = NativeIoFaultGuard::fail_on(point, nth);
            let error = stage
                .apply_access_policy(policy)
                .expect_err("late native fault must prevent a confirmed access outcome");
            assert!(fault.fired(), "selected root checkpoint must execute");
            drop(fault);
            assert_eq!(LocalFileErrorKind::Io, error.cause().kind());
            assert_eq!(LocalFileOperation::Metadata, error.cause().operation());
            assert_eq!(
                "injected native I/O contract failure",
                error.cause().io_error().expect("native source retained").to_string()
            );
            let read_only = policy == LocalTreeFinalAccess::ReadOnlyCurrentUser;
            let expected_file_mode = if read_only { 0o400 } else { 0o600 };
            let expected_root_mode = if read_only { 0o500 } else { 0o700 };
            if point == NativeIoPoint::CloneAuthority {
                assert_eq!(LocalTreeAccessState::Mixed, error.access_state());
                assert_eq!(Some(native.as_path()), error.cause().path());
                assert_eq!(
                    0o755,
                    fs::metadata(&native)
                        .expect("root not yet mutated")
                        .permissions()
                        .mode()
                        & 0o777
                );
            } else {
                assert_eq!(LocalTreeAccessState::Indeterminate, error.access_state());
                assert!(
                    error.cause().path().is_none(),
                    "same-handle observation carries no namespace path"
                );
                assert_eq!(
                    expected_root_mode,
                    fs::metadata(&native)
                        .expect("root already mutated")
                        .permissions()
                        .mode()
                        & 0o777
                );
            }
            assert_eq!(
                expected_file_mode,
                fs::metadata(native.join("payload"))
                    .expect("child already mutated")
                    .permissions()
                    .mode()
                    & 0o777
            );
            assert_eq!(
                b"original",
                fs::read(native.join("payload")).expect("original contents").as_slice()
            );
            let outcome = stage
                .apply_access_policy(policy)
                .expect("retry confirms requested access once native fault is dropped");
            assert_eq!(
                if read_only {
                    LocalTreeAccessState::ReadOnlyCurrentUser
                } else {
                    LocalTreeAccessState::CurrentUserOnly
                },
                outcome.access_state()
            );
            assert_eq!(
                expected_root_mode,
                fs::metadata(&native)
                    .expect("retried root metadata")
                    .permissions()
                    .mode()
                    & 0o777
            );
            stage
                .apply_access_policy(LocalTreeFinalAccess::CurrentUserOnly)
                .expect("restore writable fixture before cleanup");
            stage.cleanup().expect("cleanup retry owns original identity");
            assert!(!native.exists());
        }
    }
}
