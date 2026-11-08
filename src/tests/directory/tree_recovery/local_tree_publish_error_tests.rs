// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(target_os = "linux")]

use std::error::Error;
use std::path::Path;

use tempfile::tempdir;

use crate::LocalFileError;
use crate::LocalFileSystem;
use crate::directory::LocalDirectoryHandle;
use crate::directory::LocalStagedDirectory;
use crate::error::LocalFileErrorKind;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempCleanupLimits;
use crate::options::LocalTempDirectoryOptions;
use crate::outcome::LocalTreeAccessState;
use crate::policy::LocalOwnerAccessPolicy;

fn parent(path: &Path) -> LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(path, &LocalDirectoryOpenOptions::new())
        .expect("fixture parent authority")
}

fn stage(parent: &LocalDirectoryHandle) -> LocalStagedDirectory {
    let mut stage = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new()
                .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600)),
        )
        .expect("stage with owner policy");
    stage
        .write_file_new(Path::new("payload"), &mut &b"tracked"[..], 7)
        .expect("tracked contents");
    stage
}

#[test]
fn test_real_cleanup_error_source_and_parts_preserve_retry_ownership() {
    let scratch = tempdir().expect("scratch");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent);
    let path = stage.directory().diagnostic_path().to_path_buf();
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0));
    let error = stage.cleanup().expect_err("tracked entry exceeds cleanup budget");
    assert_eq!(1, error.retained_resources().len());
    assert_eq!(LocalFileErrorKind::ResourceLimit, error.cause().kind());
    assert_eq!(format!("tree cleanup failed: {}", error.cause()), error.to_string());
    assert!(std::ptr::eq(
        error
            .source()
            .expect("source")
            .downcast_ref::<LocalFileError>()
            .expect("structured cause"),
        error.cause()
    ));
    let (cause, mut owners) = error.into_parts();
    assert_eq!(LocalFileErrorKind::ResourceLimit, cause.kind());
    assert!(
        path.join("payload").exists(),
        "transferring recovery must not clean it early"
    );
    let mut owner = owners.pop().expect("retry owner");
    owner.set_cleanup_limits(LocalTempCleanupLimits::new());
    owner.cleanup().expect("retry after increasing cleanup budget");
    assert!(!path.exists());
}

#[test]
fn test_real_access_error_source_and_retained_extraction_leave_stage_usable() {
    let scratch = tempdir().expect("scratch");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent);
    let error = stage
        .apply_owner_access_policy_with_options(
            &LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600),
            &LocalListOptions::new().with_max_entries(0),
        )
        .expect_err("owner policy cannot collect an entry under a zero-entry budget");
    assert_eq!(LocalTreeAccessState::Preserved, error.access_state());
    assert_eq!(
        format!("tree access policy failed: {}", error.cause()),
        error.to_string()
    );
    assert!(std::ptr::eq(
        error
            .source()
            .expect("source")
            .downcast_ref::<LocalFileError>()
            .expect("structured cause"),
        error.cause()
    ));
    assert!(
        error.into_retained_resources().is_empty(),
        "the borrowed stage retains its owner"
    );
    stage
        .cleanup()
        .expect("access error did not transfer or disarm the stage");
}
