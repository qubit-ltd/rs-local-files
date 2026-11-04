// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Windows Rooted staging cleanup race regression tests.

#![cfg(windows)]

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalWriteFailureState;
use qubit_local_files::outcome::LocalWriterState;
use tempfile::tempdir;

const COMPETITOR_BYTES: &[u8] = b"replacement at the former staging name";
const STAGED_BYTES: &[u8] = b"original staging contents";
const SENTINEL_BYTES: &[u8] = b"unrelated sentinel";

fn directory_entries(directory: &Path) -> HashSet<std::ffi::OsString> {
    fs::read_dir(directory)
        .expect("fixture directory should be readable")
        .map(|entry| entry.expect("directory entry should be readable").file_name())
        .collect()
}

fn unique_new_entry(before: &HashSet<std::ffi::OsString>, directory: &Path) -> PathBuf {
    let after = directory_entries(directory);
    let added = after.difference(before).collect::<Vec<_>>();
    assert_eq!(
        1,
        added.len(),
        "opening the writer should add exactly one staging entry"
    );
    directory.join(added[0])
}

fn create_competing_staging_name(directory: &Path, staging: &Path) -> (PathBuf, PathBuf) {
    let moved_staging = directory.join("moved-staging");
    fs::rename(staging, &moved_staging)
        .expect("open staging entry should be renameable within the controlled directory");
    fs::write(staging, COMPETITOR_BYTES).expect("competitor should occupy the former staging name");
    (staging.to_path_buf(), moved_staging)
}

fn assert_cleanup_result(directory: &Path, old_staging_name: &Path, moved_staging: &Path, sentinel: &Path) {
    assert_eq!(
        COMPETITOR_BYTES,
        fs::read(old_staging_name)
            .expect("competitor at the former staging name must remain")
            .as_slice(),
        "cleanup must not delete or alter the competitor"
    );
    assert_eq!(
        SENTINEL_BYTES,
        fs::read(sentinel).expect("sentinel should remain").as_slice()
    );
    match fs::read(moved_staging) {
        Ok(bytes) => assert_eq!(
            STAGED_BYTES,
            bytes.as_slice(),
            "retained staging identity must be unchanged"
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => panic!("moved staging entry should be readable or absent: {error}"),
    }
    let names = directory_entries(directory);
    assert!(names.contains(old_staging_name.file_name().expect("old staging name has a filename")));
    assert!(names.contains(sentinel.file_name().expect("sentinel has a filename")));
}

/// Aborting a renamed Rooted staging entry preserves a competitor at its old
/// name.
#[test]
fn rooted_staging_abort_preserves_competitor_at_old_name() {
    let directory = tempdir().expect("temporary directory should be created");
    let sentinel = directory.path().join("sentinel");
    fs::write(&sentinel, SENTINEL_BYTES).expect("sentinel should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted authority should open");
    let before = directory_entries(directory.path());
    let mut writer = rooted
        .open_writer_with_options(Path::new("target"), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("Rooted writer should open");
    Output::write_fully(&mut writer, STAGED_BYTES).expect("staging writer should accept bytes");
    let staging = unique_new_entry(&before, directory.path());
    let (old_staging_name, moved_staging) = create_competing_staging_name(directory.path(), &staging);

    let outcome = writer.abort().expect("Rooted staging abort should complete");
    assert_eq!(LocalWriterState::Aborted, outcome.state());
    assert_cleanup_result(directory.path(), &old_staging_name, &moved_staging, &sentinel);
}

/// Dropping a renamed Rooted staging entry preserves a competitor at its old
/// name.
#[test]
fn rooted_staging_drop_preserves_competitor_at_old_name() {
    let directory = tempdir().expect("temporary directory should be created");
    let sentinel = directory.path().join("sentinel");
    fs::write(&sentinel, SENTINEL_BYTES).expect("sentinel should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted authority should open");
    let before = directory_entries(directory.path());
    let mut writer = rooted
        .open_writer_with_options(Path::new("target"), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("Rooted writer should open");
    Output::write_fully(&mut writer, STAGED_BYTES).expect("staging writer should accept bytes");
    let staging = unique_new_entry(&before, directory.path());
    let (old_staging_name, moved_staging) = create_competing_staging_name(directory.path(), &staging);

    drop(writer);
    assert_cleanup_result(directory.path(), &old_staging_name, &moved_staging, &sentinel);
}

/// Cleaning up a retained writer after a CreateNew conflict preserves the
/// old-name competitor.
#[test]
fn rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name() {
    let directory = tempdir().expect("temporary directory should be created");
    let sentinel = directory.path().join("sentinel");
    let target = directory.path().join("target");
    fs::write(&sentinel, SENTINEL_BYTES).expect("sentinel should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted authority should open");
    let before = directory_entries(directory.path());
    let mut writer = rooted
        .open_writer_with_options(Path::new("target"), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("Rooted writer should open before CreateNew commit conflict");
    Output::write_fully(&mut writer, STAGED_BYTES).expect("staging writer should accept bytes");
    let staging = unique_new_entry(&before, directory.path());
    let (old_staging_name, moved_staging) = create_competing_staging_name(directory.path(), &staging);
    fs::write(&target, b"preexisting target").expect("commit-time competing target should be written");

    let failure = writer
        .commit()
        .expect_err("existing target should reject CreateNew commit");
    assert_eq!(LocalWriteFailureState::NotPublished, failure.state());
    let (error, _, retained) = failure.into_parts();
    assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        Some(ErrorKind::AlreadyExists),
        error.io_error().map(std::io::Error::kind)
    );
    let outcome = retained
        .expect("pre-publication failure should retain its writer")
        .abort()
        .expect("retained Rooted staging should abort");
    assert_eq!(LocalWriterState::Aborted, outcome.state());
    assert_cleanup_result(directory.path(), &old_staging_name, &moved_staging, &sentinel);
    assert_eq!(
        b"preexisting target",
        fs::read(&target).expect("preexisting target should remain").as_slice()
    );
}
