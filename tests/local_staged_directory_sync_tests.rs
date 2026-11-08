// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::policy::LocalFileSyncRequirement;

#[test]
fn test_tree_sync_deadline_rejects_checked_add_overflow() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let error = stage
        .sync_tree_with_options(
            &parent,
            &LocalListOptions::new().with_deadline(std::time::Duration::MAX),
        )
        .expect_err("deadline overflow is invalid");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    stage.cleanup().expect("cleanup stage");
}

#[test]
fn test_not_required_file_sync_skips_traversal_even_after_deadline() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let result = stage
        .sync_files_with_options(
            &LocalListOptions::new().with_deadline(std::time::Duration::ZERO),
            LocalFileSyncRequirement::NotRequired,
        )
        .expect("NotRequired must not traverse or check a deadline");
    assert!(!result);
    stage.cleanup().expect("cleanup stage");
}
