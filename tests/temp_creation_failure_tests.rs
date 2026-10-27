// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "test-support")]

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

fn parent_entries(parent: &Path) -> Vec<String> {
    let mut entries = fs::read_dir(parent)
        .expect("parent should remain readable")
        .map(|entry| {
            entry
                .expect("parent entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn test_host_temp_owner_identity_failure_cleans_only_new_sandbox() {
    for (fault, create) in [("temp-identity-file", true), ("temp-identity-directory", false)] {
        let parent = tempdir().expect("parent should be created");
        fs::write(parent.path().join("existing"), b"keep").expect("existing file should be written");
        let before = parent_entries(parent.path());
        let _fault = install_test_fault(fault).expect("identity fault should install");
        let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
        let error = if create {
            filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
                .expect_err("file owner identity capture should fail")
        } else {
            filesystem
                .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(parent.path()))
                .expect_err("directory owner identity capture should fail")
        };

        assert!(error.to_string().contains("injected temporary"));
        assert_eq!(before, parent_entries(parent.path()));
        assert_eq!(
            fs::read(parent.path().join("existing")).expect("existing file should remain"),
            b"keep"
        );
    }
}

#[test]
fn test_host_temp_identity_failure_reports_sandbox_cleanup_failure() {
    let parent = tempdir().expect("parent should be created");
    fs::write(parent.path().join("existing"), b"keep").expect("existing file should be written");
    let before = parent_entries(parent.path());
    let _fault = install_test_fault("temp-identity-file-cleanup").expect("combined fault should install");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let error = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect_err("file owner identity capture should fail");
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains("injected temporary file identity failure"),
        "primary identity error should remain: {diagnostic}"
    );
    assert!(
        diagnostic.contains("failed to remove temporary sandbox"),
        "cleanup failure should be visible: {diagnostic}"
    );
    assert!(
        diagnostic.contains("injected sandbox cleanup failure"),
        "cleanup cause should be visible: {diagnostic}"
    );
    assert!(
        diagnostic.contains("sandbox-"),
        "sandbox location should be visible: {diagnostic}"
    );
    let after = parent_entries(parent.path());
    assert_eq!(before.len() + 1, after.len());
    let sandbox = after
        .iter()
        .find(|entry| entry.starts_with("sandbox-"))
        .expect("failed cleanup should leave this sandbox");
    let sandbox_path = std::path::absolute(parent.path())
        .expect("parent path should resolve absolutely")
        .join(sandbox);
    assert!(diagnostic.contains(&sandbox_path.display().to_string()));
    let contents = fs::read_dir(&sandbox_path)
        .expect("sandbox should remain inspectable")
        .collect::<Result<Vec<_>, _>>()
        .expect("sandbox entry should be readable");
    assert_eq!(1, contents.len());
    assert!(
        contents[0]
            .file_type()
            .expect("entry type should be readable")
            .is_file()
    );
}
