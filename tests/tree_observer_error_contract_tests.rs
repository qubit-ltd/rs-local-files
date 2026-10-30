// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
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
use qubit_local_files::options::LocalWalkErrorPolicy;
use tempfile::TempDir;

fn setup() -> (TempDir, LocalStagedDirectory) {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("one"), &mut &b"1"[..], 1)
        .expect("first entry");
    stage
        .write_file_new(Path::new("two"), &mut &b"2"[..], 1)
        .expect("second entry");
    (scratch, stage)
}

#[test]
fn test_entry_budget_error_terminates_observer() {
    let (_scratch, stage) = setup();
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new().with_max_entries(0))
        .expect("observer");

    assert!(matches!(observer.next(), Some(Err(ref error)) if error.kind() == LocalFileErrorKind::ResourceLimit));
    assert!(observer.next().is_none());
    drop(observer);
    stage.cleanup().expect("stage cleanup");
}

#[test]
fn test_continue_policy_does_not_bypass_global_entry_budget() {
    let (_scratch, stage) = setup();
    let limits = LocalListOptions::new()
        .with_max_entries(0)
        .with_error_policy(LocalWalkErrorPolicy::Continue);
    let mut observer = stage.observe_tree_with_options(&limits).expect("observer");

    assert!(matches!(observer.next(), Some(Err(ref error)) if error.kind() == LocalFileErrorKind::ResourceLimit));
    assert!(observer.next().is_none());
    drop(observer);
    stage.cleanup().expect("stage cleanup");
}
