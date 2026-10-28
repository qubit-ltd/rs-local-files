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
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
// =============================================================================
#![cfg(all(feature = "test-support", unix))]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

fn copy_options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_deadline(Duration::from_secs(60))
}

#[test]
fn test_rooted_file_replacement_uses_copy_deadline_during_tree_removal() {
    for (checkpoint, expected_state, child_survives) in [
        (2, LocalCopyFailureState::Unchanged, true),
        (8, LocalCopyFailureState::PartiallyPublished, false),
    ] {
        let directory = tempdir().expect("fixture should be created");
        fs::write(directory.path().join("source"), b"replacement").expect("source should be written");
        fs::create_dir(directory.path().join("target")).expect("target should be created");
        fs::write(directory.path().join("target/child"), b"original").expect("child should be written");
        let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
        let _fault = install_test_fault(&format!("local-delete-deadline-{checkpoint}"))
            .expect("delete deadline fault should install");

        let failure = rooted
            .copy_with_options(Path::new("source"), Path::new("target"), &copy_options())
            .expect_err("copy should stop at the injected deletion deadline");

        assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
        assert_eq!(expected_state, failure.state());
        assert_eq!(child_survives, directory.path().join("target/child").exists());
        assert!(!directory.path().join("target").join("source").exists());
    }
}

#[test]
fn test_host_symlink_replacement_uses_copy_deadline_during_tree_removal() {
    for (checkpoint, expected_state, child_survives) in [
        (2, LocalCopyFailureState::Unchanged, true),
        (8, LocalCopyFailureState::PartiallyPublished, false),
    ] {
        let directory = tempdir().expect("fixture should be created");
        symlink("link-target", directory.path().join("source")).expect("source link should be created");
        fs::create_dir(directory.path().join("target")).expect("target should be created");
        fs::write(directory.path().join("target/child"), b"original").expect("child should be written");
        let host = LocalFileSystem::host().expect("Host filesystem should open");
        let _fault = install_test_fault(&format!("local-delete-deadline-{checkpoint}"))
            .expect("delete deadline fault should install");

        let failure = host
            .copy_with_options(
                &directory.path().join("source"),
                &directory.path().join("target"),
                &copy_options(),
            )
            .expect_err("copy should stop at the injected deletion deadline");

        assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
        assert_eq!(expected_state, failure.state());
        assert_eq!(child_survives, directory.path().join("target/child").exists());
    }
}

#[test]
fn test_rooted_symlink_replacement_uses_copy_deadline_during_tree_removal() {
    for (checkpoint, expected_state, child_survives) in [
        (2, LocalCopyFailureState::Unchanged, true),
        (8, LocalCopyFailureState::PartiallyPublished, false),
    ] {
        let directory = tempdir().expect("fixture should be created");
        symlink("link-target", directory.path().join("source")).expect("source link should be created");
        fs::create_dir(directory.path().join("target")).expect("target should be created");
        fs::write(directory.path().join("target/child"), b"original").expect("child should be written");
        let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
        let _fault = install_test_fault(&format!("local-delete-deadline-{checkpoint}"))
            .expect("delete deadline fault should install");

        let failure = rooted
            .copy_with_options(Path::new("source"), Path::new("target"), &copy_options())
            .expect_err("copy should stop at the injected deletion deadline");

        assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
        assert_eq!(expected_state, failure.state());
        assert_eq!(child_survives, directory.path().join("target/child").exists());
    }
}
