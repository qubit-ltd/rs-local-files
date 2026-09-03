// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "test-support")]

use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

#[derive(Clone, Copy, Debug)]
enum Backend {
    Host,
    Rooted,
}

fn run_in_test_fault_process<F>(_test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    let _fault = install_test_fault(fault).expect("test fault controller should install");
    action();
}

fn filesystem_and_path(backend: Backend, root: &Path, relative: &Path) -> (LocalFileSystem, PathBuf, PathBuf) {
    match backend {
        Backend::Host => (
            LocalFileSystem::host().expect("Host filesystem should open"),
            root.join(relative),
            root.join(relative),
        ),
        Backend::Rooted => (
            LocalFileSystem::rooted(root).expect("Rooted filesystem should open"),
            Path::new(std::path::MAIN_SEPARATOR_STR).join(relative),
            root.join(relative),
        ),
    }
}

#[test]
fn recursive_create_reports_the_first_unfinished_path_after_partial_publication() {
    const TEST_NAME: &str = "recursive_create_reports_the_first_unfinished_path_after_partial_publication";
    for (backend, fault) in [
        (Backend::Host, "host-create-directory-component-second"),
        (Backend::Rooted, "rooted-create-directory-component-second"),
    ] {
        run_in_test_fault_process(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let relative = Path::new("created/blocked/target");
            let (filesystem, target, native_target) = filesystem_and_path(backend, directory.path(), relative);

            let error = filesystem
                .create_directory_with_options(&target, &LocalCreateDirectoryOptions::new().with_recursive())
                .expect_err("the second directory creation should fail");

            assert_eq!(LocalFileErrorKind::PublicationIncomplete, error.kind());
            let expected_failed = match backend {
                Backend::Host => directory.path().join("created/blocked"),
                Backend::Rooted => PathBuf::from("/created/blocked"),
            };
            assert_eq!(Some(expected_failed.as_path()), error.path());
            assert!(directory.path().join("created").is_dir());
            assert!(!native_target.exists());
        });
    }
}

#[test]
fn recursive_delete_reports_the_failed_path_after_partial_publication() {
    const TEST_NAME: &str = "recursive_delete_reports_the_failed_path_after_partial_publication";
    for (backend, fault) in [
        (Backend::Host, "host-delete-directory-entry-second"),
        (Backend::Rooted, "rooted-delete-directory-entry-second"),
    ] {
        run_in_test_fault_process(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let tree = directory.path().join("tree");
            std::fs::create_dir(&tree).expect("tree should be created");
            std::fs::write(tree.join("first"), b"first").expect("first file should be written");
            std::fs::write(tree.join("second"), b"second").expect("second file should be written");
            let (filesystem, target, _) = filesystem_and_path(backend, directory.path(), Path::new("tree"));

            let error = filesystem
                .delete_directory_with_options(&target, &LocalDeleteOptions::new().with_recursive())
                .expect_err("the second recursive removal should fail");

            assert_eq!(LocalFileErrorKind::PublicationIncomplete, error.kind());
            let failed = error.path().expect("the failed entry path should be retained");
            assert!(failed.starts_with(&target));
            assert!(tree.is_dir());
            assert_eq!(1, std::fs::read_dir(&tree).expect("tree should remain").count());
        });
    }
}
