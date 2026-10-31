// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "test-support")]

use std::error::Error;
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
    for create_file in [true, false] {
        let parent = tempdir().expect("parent should be created");
        fs::write(parent.path().join("existing"), b"keep").expect("existing file should be written");
        let before = parent_entries(parent.path());
        let fault_guard = install_test_fault("temp-identity-file-cleanup").expect("combined fault should install");
        let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
        let error = if create_file {
            filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
                .expect_err("file owner identity capture should fail")
        } else {
            filesystem
                .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(parent.path()))
                .expect_err("directory owner identity capture should fail")
        };
        assert!(error.cleanup_error().is_some(), "cleanup failure must be structured");
        assert!(
            error.temp_recovery().is_some(),
            "residual cleanup ownership must survive"
        );
        assert!(
            error.to_string().contains("injected temporary"),
            "primary identity error should remain: {error}"
        );
        let cleanup = error.cleanup_error().expect("structured cleanup error");
        let after = parent_entries(parent.path());
        assert_eq!(before.len() + 1, after.len());
        let sandbox = after
            .iter()
            .find(|entry| entry.starts_with("sandbox-"))
            .expect("failed cleanup should leave sandbox");
        let sandbox_path = std::path::absolute(parent.path())
            .expect("parent path should resolve absolutely")
            .join(sandbox);
        assert_eq!(
            fs::canonicalize(cleanup.path().expect("cleanup path should be retained"))
                .expect("retained cleanup path should exist"),
            fs::canonicalize(&sandbox_path).expect("sandbox should exist")
        );
        assert!(cleanup.to_string().contains("injected sandbox cleanup failure"));
        let (_cause, recovery) = error.into_temp_recovery_parts();
        let mut recovery = recovery.expect("retained owner");
        assert!(recovery.path().starts_with(parent.path()));
        assert!(recovery.identity().is_some(), "created entry identity must be retained");
        drop(fault_guard);
        recovery.cleanup().expect("retry after removing injected cleanup fault");
        recovery.cleanup().expect("completed cleanup is idempotent");
        assert_eq!(before, parent_entries(parent.path()));
        assert_eq!(
            fs::read(parent.path().join("existing")).expect("sibling remains"),
            b"keep"
        );
    }
}

#[test]
fn test_rooted_temp_creation_failure_reports_cleanup_failure() {
    for (fault, create_file) in [
        ("rooted-temp-file-open-cleanup", true),
        ("rooted-temp-directory-create-cleanup", false),
    ] {
        let parent = tempdir().expect("parent should be created");
        let filesystem = LocalFileSystem::rooted(parent.path()).expect("rooted filesystem should open");
        let _fault = install_test_fault(fault).expect("combined fault should install");
        let error = if create_file {
            filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new())
                .expect_err("file creation should fail")
        } else {
            filesystem
                .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
                .expect_err("directory creation should fail")
        };
        assert!(error.to_string().contains("PermissionDenied"), "{}", error);
        assert_rooted_cleanup_failure(&error, parent.path());
    }
}

#[test]
fn test_rooted_temp_owner_failure_reports_cleanup_failure() {
    for (create_file, fault, primary) in [
        (
            true,
            "rooted-temp-file-owner-cleanup",
            "file owner construction failure",
        ),
        (
            false,
            "rooted-temp-directory-owner-cleanup",
            "directory owner construction failure",
        ),
    ] {
        let parent = tempdir().expect("parent should be created");
        let filesystem = LocalFileSystem::rooted(parent.path()).expect("rooted filesystem should open");
        let _fault = install_test_fault(fault).expect("combined fault should install");
        let error = if create_file {
            filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new())
                .expect_err("file owner construction should fail")
        } else {
            filesystem
                .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
                .expect_err("directory owner construction should fail")
        };
        assert!(error.to_string().contains(primary), "{}", error);
        assert_rooted_cleanup_failure(&error, parent.path());
        let entries = parent_entries(parent.path());
        assert_eq!(1, entries.len(), "failed owner cleanup must retain its sandbox");
        let sandbox = parent.path().join(&entries[0]);
        assert_eq!(
            1,
            fs::read_dir(sandbox)
                .expect("retained sandbox should be readable")
                .count()
        );
    }
}

#[test]
fn test_rooted_temp_collision_stops_when_sandbox_cleanup_fails() {
    for (fault, create_file) in [
        ("rooted-temp-file-collision-cleanup", true),
        ("rooted-temp-directory-collision-cleanup", false),
    ] {
        let parent = tempdir().expect("parent should be created");
        let filesystem = LocalFileSystem::rooted(parent.path()).expect("rooted filesystem should open");
        let options_file = LocalTempFileOptions::new().with_max_attempts(3);
        let options_directory = LocalTempDirectoryOptions::new().with_max_attempts(3);
        let _fault = install_test_fault(fault).expect("combined fault should install");
        let error = if create_file {
            filesystem
                .create_temp_file_with_options(&options_file)
                .expect_err("collision should fail")
        } else {
            filesystem
                .create_temp_directory_with_options(&options_directory)
                .expect_err("collision should fail")
        };
        assert!(error.to_string().contains("AlreadyExists"), "{}", error);
        assert_rooted_cleanup_failure(&error, parent.path());
        assert_eq!(
            1,
            parent_entries(parent.path()).len(),
            "failed cleanup must stop retries"
        );
    }
}

fn assert_rooted_cleanup_failure(error: &qubit_local_files::LocalFileError, root: &Path) {
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("injected sandbox cleanup failure"), "{diagnostic}");
    let sandbox = error.source().expect("aggregated source should be retained");
    assert!(sandbox.to_string().contains("injected sandbox cleanup failure"));
    let primary = sandbox.source().expect("primary I/O error should be retained");
    let primary = primary.downcast_ref::<std::io::Error>().expect("native primary error");
    assert!(matches!(
        primary.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::Other
    ));
    let namespace_sandbox_prefix = format!("{}sandbox-", std::path::MAIN_SEPARATOR);
    assert!(
        diagnostic.contains(&namespace_sandbox_prefix),
        "sandbox must use rooted namespace coordinates: {diagnostic}"
    );
    assert!(
        !diagnostic.contains(&root.display().to_string()),
        "physical root must not leak: {diagnostic}"
    );
}
