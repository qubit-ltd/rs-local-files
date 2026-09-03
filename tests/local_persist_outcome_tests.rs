// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public persistence outcome coverage.

#[cfg(unix)]
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::symlink;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalPersistCleanupState;
use qubit_local_files::outcome::LocalPersistMethod;
#[cfg(feature = "test-support")]
use qubit_local_files::test_support::install_test_fault;

#[cfg(feature = "test-support")]
fn run_in_test_fault_process<F>(_test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    let _fault = install_test_fault(fault).expect("test fault controller should install");
    action();
}

#[cfg(unix)]
use qubit_local_files::options::LocalTempDirectoryOptions;

/// Verifies outcome accessors report the completed temporary-file publication.
#[test]
fn test_local_persist_outcome_reports_published_path_and_guarantees() {
    let root = tempfile::tempdir().expect("test root must be created");
    let target = root.path().join("target.txt");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(root.path()))
        .expect("temporary file must be created");
    temporary
        .write_all(b"payload")
        .expect("temporary file must be writable");

    let outcome = temporary
        .persist_with(&target, LocalPersistOptions::new())
        .expect("temporary file must persist");

    assert_eq!(target, outcome.path());
    assert_eq!(LocalPersistMethod::AtomicRename, outcome.method());
    assert!(outcome.atomic());
    assert!(!outcome.durable());
    assert_eq!(LocalPersistCleanupState::Complete, outcome.cleanup_state());
    assert!(outcome.cleanup_error().is_none());
    assert_eq!(target, outcome.path());
    let (published_path, cleanup_error) = outcome.into_parts();
    assert_eq!(target, published_path);
    assert!(cleanup_error.is_none());
}

/// Verifies Host persistence keeps the caller-visible path when an
/// intermediate symbolic link resolves to a different native path.
#[cfg(unix)]
#[test]
fn test_local_file_persist_outcome_preserves_logical_target_path() {
    let root = tempfile::tempdir().expect("test root must be created");
    let real_parent = root.path().join("real");
    fs::create_dir(&real_parent).expect("real parent must be created");
    let logical_parent = root.path().join("logical");
    symlink(&real_parent, &logical_parent).expect("logical parent symlink must be created");
    let target = logical_parent.join("target.txt");
    let expected = std::path::absolute(&target).expect("logical target must be made absolute");

    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(root.path()))
        .expect("temporary file must be created");
    temporary
        .write_all(b"payload")
        .expect("temporary file must be writable");

    let outcome = temporary
        .persist_with(&target, LocalPersistOptions::new())
        .expect("temporary file must persist");

    assert_eq!(expected, outcome.path());
    assert_eq!(fs::read(&target).expect("target must be readable"), b"payload",);
}

/// Verifies Host temporary-directory persistence keeps the logical target
/// path when an intermediate symbolic link resolves to another directory.
#[cfg(unix)]
#[test]
fn test_local_directory_persist_outcome_preserves_logical_target_path() {
    let root = tempfile::tempdir().expect("test root must be created");
    let real_parent = root.path().join("real");
    fs::create_dir(&real_parent).expect("real parent must be created");
    let logical_parent = root.path().join("logical");
    symlink(&real_parent, &logical_parent).expect("logical parent symlink must be created");
    let target = logical_parent.join("target");
    let expected = std::path::absolute(&target).expect("logical target must be made absolute");

    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(root.path()))
        .expect("temporary directory must be created");

    let outcome = temporary
        .persist_with(&target, LocalPersistOptions::new())
        .expect("temporary directory must persist");

    assert_eq!(expected, outcome.path());
    assert!(target.is_dir());
}

/// Verifies publication succeeds while reporting residual sandbox cleanup.
#[cfg(feature = "test-support")]
#[test]
fn test_local_persist_outcome_reports_residual_sandbox_cleanup() {
    run_in_test_fault_process(
        "test_local_persist_outcome_reports_residual_sandbox_cleanup",
        "temp-file-sandbox-remove",
        || {
            let root = tempfile::tempdir().expect("test root must be created");
            let target = root.path().join("target.txt");
            let temporary = LocalFileSystem::host()
                .expect("Host filesystem should open")
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(root.path()))
                .expect("temporary file must be created");

            let outcome = temporary
                .persist(&target)
                .expect("publication should succeed despite sandbox cleanup failure");

            assert_eq!(target, outcome.path());
            assert_eq!(LocalPersistCleanupState::ResidualSandbox, outcome.cleanup_state(),);
            assert!(outcome.cleanup_error().is_some());
            assert!(target.is_file());
        },
    );
}
