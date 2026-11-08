// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Strict depth limits for staged tree recovery cleanup.

use std::fs::canonicalize;
use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalStagedDirectory;
use qubit_local_files::directory::LocalTreeValidationOptions;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempCleanupLimits;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::outcome::LocalTreeMutationState;
use tempfile::TempDir;
use tempfile::tempdir;

fn stage_with_files(paths: &[&str]) -> (TempDir, LocalStagedDirectory, PathBuf) {
    let scratch = tempdir().expect("scratch");
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
        canonicalize(error_path).expect("reported cleanup path should resolve"),
        canonicalize(&native).expect("native cleanup path should resolve"),
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
fn test_empty_stage_cleanup_honors_path_byte_budget() {
    for rooted in [false, true] {
        for limit_kind in 0..4 {
            let scratch = tempdir().expect("scratch");
            let filesystem = if rooted {
                LocalFileSystem::rooted(scratch.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem");
            let parent_path = if rooted { Path::new("/") } else { scratch.path() };
            let parent = filesystem
                .open_directory_with_options(parent_path, &LocalDirectoryOpenOptions::new())
                .expect("parent");
            let stage = parent
                .create_staged_directory(&LocalTempDirectoryOptions::new())
                .expect("stage");
            let relative = PathBuf::from(stage.relative_name());
            let identity = stage.identity().expect("stage identity").clone();
            let location = stage.retain_in_place();
            let namespace_path = location.namespace_path().to_path_buf();
            let root_bytes = namespace_path.as_os_str().as_encoded_bytes().len();
            let native_stage = scratch.path().join(&relative);
            let maximum = match limit_kind {
                0 => Some(0),
                1 => Some(root_bytes.saturating_sub(1)),
                2 => Some(root_bytes),
                _ => None,
            };
            let mut retained = parent
                .open_staged_directory(&relative, &identity, &LocalTreeValidationOptions::new())
                .expect("stage should reopen");
            if let Some(maximum) = maximum {
                retained.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_pending_path_bytes(maximum));
            }
            if maximum.is_some_and(|maximum| maximum < root_bytes) {
                let error = retained.cleanup().expect_err("root path exceeds budget");
                assert_eq!(LocalFileErrorKind::ResourceLimit, error.cause().kind());
                assert_eq!(LocalFileOperation::Cleanup, error.cause().operation());
                assert_eq!(LocalTreeMutationState::NotStarted, error.mutation_state());
                assert_eq!(Some(namespace_path.as_path()), error.cause().path());
                assert!(native_stage.exists(), "budget failure must precede deletion");
                for mut owner in error.into_retained_resources() {
                    owner.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_pending_path_bytes(root_bytes));
                    owner.cleanup().expect("exact root budget permits cleanup");
                }
            } else {
                retained.cleanup().expect("root path fits budget");
            }
            assert!(!native_stage.exists());
        }
    }
}

#[test]
fn test_empty_stage_drop_honors_path_byte_budget() {
    for rooted in [false, true] {
        let scratch = tempdir().expect("scratch");
        let filesystem = if rooted {
            LocalFileSystem::rooted(scratch.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let parent_path = if rooted { Path::new("/") } else { scratch.path() };
        let parent = filesystem
            .open_directory_with_options(parent_path, &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        let relative = PathBuf::from(stage.relative_name());
        let identity = stage.identity().expect("stage identity").clone();
        let location = stage.retain_in_place();
        let root_bytes = location.namespace_path().as_os_str().as_encoded_bytes().len();
        let native_stage = scratch.path().join(&relative);
        let mut retained = parent
            .open_staged_directory(&relative, &identity, &LocalTreeValidationOptions::new())
            .expect("stage should reopen");
        retained.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_pending_path_bytes(root_bytes - 1));
        drop(retained);
        assert!(
            native_stage.exists(),
            "Drop must leave a stage whose root exceeds its budget"
        );
    }
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
