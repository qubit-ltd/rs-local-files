// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::io::Write;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;
use qubit_local_files::LocalWriterState;
use tempfile::tempdir;

/// Verifies shared atomic publication transitions preserve Host replacement.
#[test]
fn test_atomic_commit_state_publishes_host_staging() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");
    let mut writer = LocalFileSystem::host()
        .open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("Host writer should open");
    writer
        .write_all(b"new")
        .expect("staging should accept bytes");

    let outcome = writer.commit().expect("Host commit should publish");

    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert_eq!(
        b"new".to_vec(),
        fs::read(&target).expect("target should be readable")
    );
}

/// Verifies shared atomic publication transitions preserve rooted replacement.
#[test]
fn test_atomic_commit_state_publishes_rooted_staging() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("target"), b"old")
        .expect("target fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path())
        .expect("rooted filesystem should open");
    let mut writer = filesystem
        .open_writer(
            Path::new("target"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("rooted writer should open");
    writer
        .write_all(b"new")
        .expect("staging should accept bytes");

    let outcome = writer.commit().expect("rooted commit should publish");

    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert_eq!(
        b"new".to_vec(),
        fs::read(directory.path().join("target"))
            .expect("target should be readable")
    );
}
