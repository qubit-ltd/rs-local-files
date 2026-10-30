// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Strict depth limits for staged tree recovery cleanup.

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempCleanupLimits;
use qubit_local_files::options::LocalTempDirectoryOptions;

fn stage_with_files(
    paths: &[&str],
) -> (
    tempfile::TempDir,
    qubit_local_files::directory::LocalStagedDirectory,
    std::path::PathBuf,
) {
    let scratch = tempfile::tempdir().expect("scratch");
    let fs = LocalFileSystem::host().expect("host");
    let parent = fs
        .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    for path in paths {
        let path = Path::new(path);
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            let _directory = stage
                .directory()
                .create_directory_with_options(parent, &LocalCreateDirectoryOptions::new().with_recursive())
                .expect("nested directory");
        }
        stage.write_file_new(path, &mut &b"x"[..], 8).expect("file");
    }
    let native = scratch.path().join(stage.relative_name());
    (scratch, stage, native)
}

#[test]
fn test_stage_cleanup_depth_zero_returns_typed_limit_and_can_retry() {
    let (_scratch, mut stage, native_stage) = stage_with_files(&["child"]);
    let native = native_stage.join("child");
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(0));

    let error = stage.cleanup().expect_err("depth zero");
    assert_eq!(error.cause().kind(), LocalFileErrorKind::ResourceLimit);
    let limit = error.cause().resource_limit_error().expect("typed depth");
    assert_eq!(limit.resource(), LocalResourceKind::Depth);
    assert_eq!(LocalFileOperation::Cleanup, error.cause().operation());
    assert_eq!(0, limit.limit());
    assert_eq!(1, limit.requested());
    assert_eq!(Some(1), limit.observed());
    let error_path = error.cause().path().expect("cleanup path");
    assert_eq!(
        std::fs::canonicalize(error_path).expect("reported cleanup path should resolve"),
        std::fs::canonicalize(&native).expect("native cleanup path should resolve"),
    );
    assert!(native.exists());

    for mut owner in error.into_retained_resources() {
        owner.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(1));
        owner.cleanup().expect("bounded retry");
    }
    assert!(!native.exists());
}

#[test]
fn test_empty_stage_cleanup_succeeds_at_depth_zero() {
    let (_scratch, mut stage, native) = stage_with_files(&[]);
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(0));
    stage.cleanup().expect("empty tree fits depth zero");
    assert!(!native.exists());
}

#[test]
fn test_child_cleanup_succeeds_at_depth_one() {
    let (_scratch, mut stage, native) = stage_with_files(&["child"]);
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(1));
    stage.cleanup().expect("direct child fits depth one");
    assert!(!native.exists());
}

#[test]
fn test_nested_cleanup_retries_with_larger_depth_budget() {
    let (_scratch, mut stage, native) = stage_with_files(&["nested/leaf"]);
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(1));
    let error = stage.cleanup().expect_err("nested leaf exceeds depth one");
    assert_eq!(
        LocalResourceKind::Depth,
        error.cause().resource_limit_error().expect("depth").resource()
    );
    assert!(native.join("nested/leaf").exists());

    let owner = error.into_retained_resources().pop().expect("retained owner");
    let retry = owner.cleanup().expect_err("same budget still fails");
    let mut owner = retry.into_retained_resources().pop().expect("owner retained again");
    owner.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(2));
    owner.cleanup().expect("larger bounded retry");
    assert!(!native.exists());
}

#[test]
fn test_drop_keeps_cleanup_depth_limit() {
    let (_scratch, mut stage, native) = stage_with_files(&["child"]);
    stage.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(0));
    let error = stage.cleanup().expect_err("explicit cleanup reports depth limit");
    drop(error);
    assert!(native.join("child").exists());
}
