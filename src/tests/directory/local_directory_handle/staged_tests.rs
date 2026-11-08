// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(target_os = "linux")]

use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::LocalFileSystem;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::outcome::LocalTreeMutationState;
use crate::policy::LocalOwnerAccessPolicy;

fn create_failed_stage() -> (tempfile::TempDir, crate::directory::LocalTreeCreateError) {
    let fixture = tempfile::tempdir().expect("temporary directory");
    fs::write(fixture.path().join("sentinel"), b"independent sentinel").expect("sentinel");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let parent = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("open parent");
    crate::temp::internal::privacy_unix::install_owner_open_failure_hook(|parent, name| {
        let c_name = std::ffi::CString::new(name.as_bytes()).expect("stage name contains no NUL");
        let original = Path::new("/proc/self/fd")
            .join(parent.as_raw_fd().to_string())
            .join(name);
        fs::write(original.join("payload"), b"original payload").expect("original payload");
        let moved = std::ffi::CString::new("moved-original").expect("static name");
        // SAFETY: names are NUL terminated and parent is an open directory.
        assert_eq!(
            unsafe { libc::renameat(parent.as_raw_fd(), c_name.as_ptr(), parent.as_raw_fd(), moved.as_ptr(),) },
            0
        );
        // SAFETY: same authority and a valid directory mode.
        assert_eq!(unsafe { libc::mkdirat(parent.as_raw_fd(), c_name.as_ptr(), 0o700) }, 0);
        Err(std::io::Error::other("injected directory open failure"))
    });
    let options = LocalTempDirectoryOptions::new()
        .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600));
    let error = parent
        .create_staged_directory(&options)
        .expect_err("injected open failure");
    (fixture, error)
}

#[test]
fn test_owner_policy_failed_open_does_not_arm_tree_cleanup_for_replacement() {
    let (fixture, error) = create_failed_stage();
    assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
    assert_eq!(error.cause().io_error().and_then(std::io::Error::raw_os_error), None);
    let retained_path = error.cause().path().expect("failed stage path").to_path_buf();
    let replacement = fs::metadata(&retained_path).expect("replacement metadata");
    let replacement_identity = crate::observation::LocalFileIdentity::unix(replacement.dev(), replacement.ino());
    assert!(
        error.retained_resources().is_empty(),
        "unproven failed-open entry must not arm tree cleanup"
    );
    assert!(error.cause().temp_recovery().is_some(), "native recovery is retained");
    assert_eq!(
        fs::read(fixture.path().join("moved-original/payload")).expect("moved original"),
        b"original payload"
    );
    assert_eq!(
        fs::read(fixture.path().join("sentinel")).expect("sentinel survives"),
        b"independent sentinel"
    );

    let (cause, _, owners) = error.into_parts();
    assert!(owners.is_empty(), "failed open has no verifiable tree owner");
    let (_, mut recovery) = cause.into_temp_recovery_parts();
    let mut recovery = recovery.take().expect("native recovery owner");
    assert!(recovery.identity().is_none(), "replacement identity is unproven");
    assert_ne!(Some(&replacement_identity), recovery.identity());
    assert!(recovery.cleanup().is_err(), "unknown identity must refuse cleanup");
    assert!(retained_path.is_dir(), "explicit recovery must leave replacement");
    drop(recovery);
    assert!(retained_path.is_dir(), "recovery Drop must leave replacement");
}

#[test]
fn test_owner_policy_failed_open_drop_does_not_delete_replacement() {
    let (fixture, error) = create_failed_stage();
    let retained_path = error.cause().path().expect("failed stage path").to_path_buf();
    drop(error);
    assert!(retained_path.is_dir(), "Drop must not delete the replacement directory");
    assert_eq!(
        fs::read(fixture.path().join("moved-original/payload")).expect("moved original"),
        b"original payload"
    );
    assert_eq!(
        fs::read(fixture.path().join("sentinel")).expect("sentinel survives"),
        b"independent sentinel"
    );
}
