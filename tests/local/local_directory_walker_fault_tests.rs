// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage-only fault tests for native directory walking.

use std::fs;

use qubit_local_files::{LocalFileErrorKind, LocalFileSystem, LocalListOptions};
use tempfile::tempdir;

use super::test_support::run_in_coverage_fault_process;

/// Verifies follow-mode root canonicalization failures retain the root path.
#[test]
fn test_walker_reports_injected_root_canonicalization_failure() {
    const TEST_NAME: &str = concat!(
        "local::local_directory_walker_fault_tests::",
        "test_walker_reports_injected_root_canonicalization_failure",
    );
    let Some(()) = run_in_coverage_fault_process(TEST_NAME, "walker-root-canonicalize", || {
        let root = tempdir().expect("walker root should be created");
        let error =
            LocalFileSystem::list(root.path(), &LocalListOptions::new().with_follow_symlinks())
                .expect_err("injected root canonicalization should fail");
        assert_eq!(LocalFileErrorKind::Io, error.kind());
        assert_eq!(Some(root.path()), error.path());
    }) else {
        return;
    };
}

/// Verifies follow-mode descent canonicalization failures identify the child.
#[test]
fn test_walker_reports_injected_descent_canonicalization_failure() {
    const TEST_NAME: &str = concat!(
        "local::local_directory_walker_fault_tests::",
        "test_walker_reports_injected_descent_canonicalization_failure",
    );
    let Some(()) = run_in_coverage_fault_process(TEST_NAME, "walker-descend-canonicalize", || {
        let root = tempdir().expect("walker root should be created");
        let child = root.path().join("child");
        fs::create_dir(&child).expect("child directory should be created");
        let error = LocalFileSystem::list(
            root.path(),
            &LocalListOptions::new()
                .with_recursive()
                .with_follow_symlinks(),
        )
        .expect("walker should open")
        .next()
        .expect("child entry should be observed")
        .expect_err("injected descent canonicalization should fail");
        assert_eq!(LocalFileErrorKind::Io, error.kind());
        assert_eq!(Some(child.as_path()), error.path());
    }) else {
        return;
    };
}

/// Verifies an iterator entry failure retains the current directory context.
#[test]
fn test_walker_reports_injected_directory_entry_failure() {
    const TEST_NAME: &str = concat!(
        "local::local_directory_walker_fault_tests::",
        "test_walker_reports_injected_directory_entry_failure",
    );
    let Some(()) = run_in_coverage_fault_process(TEST_NAME, "walker-entry", || {
        let root = tempdir().expect("walker root should be created");
        let error = LocalFileSystem::list(root.path(), &LocalListOptions::new())
            .expect("walker should open")
            .next()
            .expect("injected entry error should be observed")
            .expect_err("injected directory entry should fail");
        assert_eq!(LocalFileErrorKind::Io, error.kind());
        assert_eq!(Some(root.path()), error.path());
    }) else {
        return;
    };
}
