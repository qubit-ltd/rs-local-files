// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
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

fn options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_tree_source()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_deadline(Duration::from_secs(60))
}

fn assert_host_tree_copy_deadline(source_kind: SourceKind) {
    for (checkpoint, expected_state, child_survives) in [
        (2, LocalCopyFailureState::Unchanged, true),
        (9, LocalCopyFailureState::PartiallyPublished, false),
    ] {
        let directory = tempdir().expect("fixture should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::create_dir(&source).expect("source root should be created");
        fs::create_dir(&target).expect("target root should be created");
        match source_kind {
            SourceKind::File => fs::write(source.join("entry"), b"replacement").expect("source file should be written"),
            SourceKind::Symlink => symlink("link-target", source.join("entry")).expect("source link should be created"),
        }
        fs::create_dir(target.join("entry")).expect("conflicting destination directory should be created");
        fs::write(target.join("entry/child"), b"original").expect("destination child should be written");
        let host = LocalFileSystem::host().expect("Host filesystem should open");
        let _fault = install_test_fault(&format!("local-delete-deadline-{checkpoint}"))
            .expect("delete deadline fault should install");

        let failure = host
            .copy_with_options(Path::new(&source), Path::new(&target), &options())
            .expect_err("copy should stop at the injected deletion deadline");

        assert_eq!(std::io::ErrorKind::TimedOut, failure.error().io_error_kind());
        assert_eq!(expected_state, failure.state());
        assert_eq!(
            child_survives,
            target.join("entry/child").exists(),
            "checkpoint {checkpoint} should have the expected removal effect",
        );
    }
}

#[derive(Clone, Copy)]
enum SourceKind {
    File,
    Symlink,
}

#[test]
fn test_review_host_file_conflict_directory_uses_copy_deadline() {
    assert_host_tree_copy_deadline(SourceKind::File);
}

#[test]
fn test_review_host_symlink_conflict_directory_uses_copy_deadline() {
    assert_host_tree_copy_deadline(SourceKind::Symlink);
}

#[test]
fn test_review_host_conflict_directory_removal_ignores_source_limits() {
    let directory = tempdir().expect("fixture should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir(&source).expect("source root should be created");
    fs::create_dir(&target).expect("target root should be created");
    fs::write(source.join("entry"), b"x").expect("source file should be written");
    fs::create_dir_all(target.join("entry/deep/nested")).expect("deep destination should be created");
    for index in 0..8 {
        fs::write(target.join(format!("entry/deep/nested/{index}")), b"old")
            .expect("destination child should be written");
    }
    let copy_options = options().with_max_depth(1).with_max_entries(2).with_max_bytes(1);

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(Path::new(&source), Path::new(&target), &copy_options)
        .expect("source limits should not constrain destination deletion");

    assert_eq!(1, outcome.stats().files());
    assert_eq!(
        b"x".to_vec(),
        fs::read(target.join("entry")).expect("source file should replace directory")
    );
}
