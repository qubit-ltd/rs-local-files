// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    io::Write,
};

use qubit_local_files::{
    LocalAtomicityRequirement,
    LocalFileErrorKind,
    LocalFileSystem,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
};
use tempfile::tempdir;

/// Verifies staged replacement is invisible until commit.
#[test]
fn test_local_file_writer_publishes_staged_content_on_commit() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");

    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    )
    .expect("staged writer should open");
    writer
        .write_all(b"new")
        .expect("staged content should be written");
    assert_eq!(
        b"old",
        fs::read(&target)
            .expect("old target should remain")
            .as_slice()
    );

    let outcome = writer
        .commit()
        .expect("commit should publish staged content");
    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert!(outcome.atomic());
    assert_eq!(3, outcome.bytes_written());
    assert_eq!(
        b"new",
        fs::read(&target)
            .expect("target should be replaced")
            .as_slice()
    );
}

/// Verifies that overwrite publication replaces a target symlink entry.
#[cfg(unix)]
#[test]
fn test_local_file_writer_replaces_target_symlink_entry() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let referent = directory.path().join("referent");
    fs::write(&referent, b"original").expect("referent should be written");
    symlink(&referent, &target).expect("target symlink should be created");

    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = LocalFileSystem::open_writer(&target, &options)
        .expect("writer should accept a target symlink entry");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    let outcome = writer.commit().expect("replacement should publish");
    assert_eq!(LocalWriterState::Committed, outcome.state());

    assert!(
        fs::symlink_metadata(&target)
            .expect("target metadata should be available")
            .is_file(),
    );
    assert_eq!(
        b"replacement".to_vec(),
        fs::read(&target).expect("target should contain replacement"),
    );
    assert_eq!(
        b"original".to_vec(),
        fs::read(&referent).expect("referent should remain unchanged"),
    );
}

/// Verifies create-new rejects an existing entry before writing.
#[test]
fn test_local_file_writer_create_new_rejects_existing_target() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");

    let error = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateNew),
    )
    .expect_err("create-new must reject the existing target");

    assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
}

/// Verifies abort cleans staging without modifying the destination.
#[test]
fn test_local_file_writer_abort_keeps_original_target() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");
    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    )
    .expect("staged writer should open");
    writer
        .write_all(b"new")
        .expect("staging write should succeed");

    let outcome = writer.abort().expect("abort should clean staging");

    assert_eq!(LocalWriterState::Aborted, outcome.state());
    assert_eq!(
        b"old",
        fs::read(&target)
            .expect("target should remain unchanged")
            .as_slice()
    );
}

/// Verifies direct append refuses a required atomicity guarantee.
#[test]
fn test_local_file_writer_append_rejects_required_atomicity() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");

    let error = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::Append)
            .with_atomicity(LocalAtomicityRequirement::Required),
    )
    .expect_err("direct append cannot provide required atomicity");

    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.kind());
}
