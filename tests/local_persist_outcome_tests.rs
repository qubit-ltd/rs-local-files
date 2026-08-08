// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public persistence outcome coverage.

use std::io::Write;

#[cfg(unix)]
use std::fs;

#[cfg(unix)]
use std::os::unix::fs::symlink;

use qubit_local_files::{
    LocalFileSystem,
    LocalPersistCleanupState,
    LocalPersistMethod,
    LocalPersistOptions,
    LocalTempFileOptions,
};

#[cfg(feature = "internal-test-support")]
fn run_in_test_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_ENV)
        .is_some_and(|selected| selected == std::ffi::OsStr::new(fault))
    {
        let _fault = qubit_local_files::install_test_fault(fault)
            .expect("test fault controller should install");
        action();
        return;
    }
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some() {
        return;
    }
    let executable =
        std::env::current_exe().expect("test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(TEST_FAULT_ENV, fault)
        .env(TEST_FAULT_CHILD_ENV, "1")
        .status()
        .expect("test fault child should launch");
    assert!(status.success(), "test fault child should pass");
}

#[cfg(unix)]
use qubit_local_files::LocalTempDirectoryOptions;

/// Verifies outcome accessors report the completed temporary-file publication.
#[test]
fn test_local_persist_outcome_reports_published_path_and_guarantees() {
    let root = tempfile::tempdir().expect("test root must be created");
    let target = root.path().join("target.txt");
    let mut temporary = LocalFileSystem::host()
        .create_temp_file(&LocalTempFileOptions::new().with_parent(root.path()))
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
    symlink(&real_parent, &logical_parent)
        .expect("logical parent symlink must be created");
    let target = logical_parent.join("target.txt");
    let expected = std::path::absolute(&target)
        .expect("logical target must be made absolute");

    let mut temporary = LocalFileSystem::host()
        .create_temp_file(&LocalTempFileOptions::new().with_parent(root.path()))
        .expect("temporary file must be created");
    temporary
        .write_all(b"payload")
        .expect("temporary file must be writable");

    let outcome = temporary
        .persist_with(&target, LocalPersistOptions::new())
        .expect("temporary file must persist");

    assert_eq!(expected, outcome.path());
    assert_eq!(
        fs::read(&target).expect("target must be readable"),
        b"payload",
    );
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
    symlink(&real_parent, &logical_parent)
        .expect("logical parent symlink must be created");
    let target = logical_parent.join("target");
    let expected = std::path::absolute(&target)
        .expect("logical target must be made absolute");

    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(root.path()),
        )
        .expect("temporary directory must be created");

    let outcome = temporary
        .persist_with(&target, LocalPersistOptions::new())
        .expect("temporary directory must persist");

    assert_eq!(expected, outcome.path());
    assert!(target.is_dir());
}

/// Verifies publication succeeds while reporting residual sandbox cleanup.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_local_persist_outcome_reports_residual_sandbox_cleanup() {
    run_in_test_fault_process(
        "test_local_persist_outcome_reports_residual_sandbox_cleanup",
        "temp-file-sandbox-remove",
        || {
            let root = tempfile::tempdir().expect("test root must be created");
            let target = root.path().join("target.txt");
            let temporary = LocalFileSystem::host()
                .create_temp_file(
                    &LocalTempFileOptions::new().with_parent(root.path()),
                )
                .expect("temporary file must be created");

            let outcome = temporary.persist(&target).expect(
                "publication should succeed despite sandbox cleanup failure",
            );

            assert_eq!(target, outcome.path());
            assert_eq!(
                LocalPersistCleanupState::ResidualSandbox,
                outcome.cleanup_state(),
            );
            assert!(outcome.cleanup_error().is_some());
            assert!(target.is_file());
        },
    );
}
