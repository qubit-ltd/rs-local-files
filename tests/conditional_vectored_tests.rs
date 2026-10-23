// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::policy::LocalSymlinkPolicy;
use sha2::Digest;
use sha2::Sha256;

#[test]
fn aborted_conditional_writer_rejects_empty_vectors() {
    let dir = tempfile::tempdir().unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(LocalSymlinkPolicy::Reject).unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &dir.path().join("cancelled"),
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();
    assert_eq!(
        writer.abort().unwrap().state(),
        qubit_local_files::outcome::LocalWriterState::Aborted
    );
    assert_eq!(
        Output::write_vectored(&mut writer, &[]).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
}

#[test]
fn mixed_scalar_and_vectored_writes_publish_expected_digest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mixed");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(LocalSymlinkPolicy::Reject).unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();
    writer.write_fully(b"a").unwrap();
    let written = Output::write_vectored(&mut writer, &[b"b", b"c"]).unwrap();
    assert!((1..=2).contains(&written));
    if written < 2 {
        writer.write_fully(b"c").unwrap();
    }
    let saved = writer.commit().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"abc");
    assert_eq!(saved.digest(), &Sha256::digest(b"abc"));
    assert_eq!(saved.write_outcome().bytes_written(), 3);
}

#[test]
fn active_empty_vectors_return_zero_without_changing_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty-vectors");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(LocalSymlinkPolicy::Reject).unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();
    assert_eq!(Output::write_vectored(&mut writer, &[]).unwrap(), 0);
    assert_eq!(Output::write_vectored(&mut writer, &[b"", b""]).unwrap(), 0);
    writer.write_fully(b"x").unwrap();
    assert_eq!(Output::write_vectored(&mut writer, &[]).unwrap(), 0);
    let saved = writer.commit().unwrap();
    assert_eq!(saved.write_outcome().bytes_written(), 1);
    assert_eq!(std::fs::read(path).unwrap(), b"x");
}
