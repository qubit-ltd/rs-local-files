// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(all(feature = "test-support", unix))]

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

const PAYLOAD: &[u8] = b"source payload";
const SENTINEL: &[u8] = b"concurrent sentinel";

fn assert_no_staging_files(directory: &Path) {
    let entries = fs::read_dir(directory)
        .expect("copy fixture should be readable")
        .map(|entry| entry.expect("fixture entry should be readable").file_name())
        .collect::<Vec<_>>();
    assert_eq!(2, entries.len(), "only source and target should remain: {entries:?}");
    assert!(entries.iter().any(|name| name == "source"));
    assert!(entries.iter().any(|name| name == "target"));
}

fn setup_copy() -> (tempfile::TempDir, LocalFileSystem) {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), PAYLOAD).expect("source should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    (directory, rooted)
}

fn options(policy: LocalCopyConflictPolicy) -> LocalCopyOptions {
    LocalCopyOptions::new().with_conflict(policy)
}

fn assert_race_failure_preserves_sentinel(selector: &str) {
    let (directory, rooted) = setup_copy();
    let _fault = install_test_fault(selector).expect("race selector should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &options(LocalCopyConflictPolicy::Fail),
        )
        .expect_err("Fail must report the concurrent destination");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(LocalFileErrorKind::AlreadyExists, failure.error().kind());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
    assert_eq!(0, failure.partial_stats().skipped());
    assert_eq!(0, failure.partial_stats().overwritten());
    assert!(failure.cleanup_error().is_none());
    assert_eq!(SENTINEL, fs::read(directory.path().join("target")).unwrap().as_slice());
    assert_no_staging_files(directory.path());
}

fn assert_race_skip_preserves_sentinel(selector: &str) {
    let (directory, rooted) = setup_copy();
    let _fault = install_test_fault(selector).expect("race selector should install");

    let outcome = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &options(LocalCopyConflictPolicy::Skip),
        )
        .expect("Skip should preserve the concurrent destination");

    assert_eq!(1, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().files());
    assert_eq!(0, outcome.stats().bytes());
    assert_eq!(0, outcome.stats().overwritten());
    assert_eq!(SENTINEL, fs::read(directory.path().join("target")).unwrap().as_slice());
    assert_no_staging_files(directory.path());
}

fn assert_race_overwrite_replaces_sentinel(selector: &str) {
    let (directory, rooted) = setup_copy();
    let _fault = install_test_fault(selector).expect("race selector should install");

    let outcome = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &options(LocalCopyConflictPolicy::Overwrite),
        )
        .expect("Overwrite should publish the copied file");

    assert_eq!(1, outcome.stats().files());
    assert_eq!(PAYLOAD.len() as u64, outcome.stats().bytes());
    assert_eq!(0, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().overwritten());
    assert_eq!(PAYLOAD, fs::read(directory.path().join("target")).unwrap().as_slice());
    assert_no_staging_files(directory.path());
}

#[test]
fn test_before_writer_open_fail_preserves_concurrent_destination() {
    assert_race_failure_preserves_sentinel("rooted-copy-race-before-writer-open");
}

#[test]
fn test_before_writer_open_skip_counts_only_skipped_entry() {
    assert_race_skip_preserves_sentinel("rooted-copy-race-before-writer-open");
}

#[test]
fn test_before_writer_open_overwrite_replaces_concurrent_destination() {
    assert_race_overwrite_replaces_sentinel("rooted-copy-race-before-writer-open");
}

#[test]
fn test_before_writer_commit_fail_preserves_concurrent_destination() {
    assert_race_failure_preserves_sentinel("rooted-copy-race-before-writer-commit");
}

#[test]
fn test_before_writer_commit_skip_counts_only_skipped_entry() {
    assert_race_skip_preserves_sentinel("rooted-copy-race-before-writer-commit");
}

#[test]
fn test_before_writer_commit_overwrite_preserves_competitor_after_open() {
    let (directory, rooted) = setup_copy();
    let _fault = install_test_fault("rooted-copy-race-before-writer-commit").expect("race selector should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &options(LocalCopyConflictPolicy::Overwrite),
        )
        .expect_err("a competitor created after writer initialization must remain protected");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(LocalFileErrorKind::AlreadyExists, failure.error().kind());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
    assert_eq!(0, failure.partial_stats().skipped());
    assert_eq!(0, failure.partial_stats().overwritten());
    assert_eq!(SENTINEL, fs::read(directory.path().join("target")).unwrap().as_slice());
    assert_no_staging_files(directory.path());
}

#[test]
fn test_file_skip_policy_does_not_skip_a_directory_type_conflict() {
    let (directory, rooted) = setup_copy();
    fs::create_dir(directory.path().join("target")).expect("conflicting directory should be created");
    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Skip)
                .with_type_conflict(qubit_local_files::options::LocalCopyTypeConflictPolicy::Fail),
        )
        .expect_err("file Skip must not override an explicit type-conflict Fail");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(LocalFileErrorKind::AlreadyExists, failure.error().kind());
    assert!(directory.path().join("target").is_dir());
    assert_eq!(0, failure.partial_stats().skipped());
    assert_eq!(0, failure.partial_stats().files());
    assert_no_staging_files(directory.path());
}

#[test]
fn test_commit_cleanup_failure_is_not_converted_to_skip() {
    let (directory, rooted) = setup_copy();
    let _fault = install_test_fault("rooted-copy-install-cleanup").expect("cleanup fault should install");

    let failure = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &options(LocalCopyConflictPolicy::Skip),
        )
        .expect_err("a commit conflict with failed cleanup must remain an error");

    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
    assert_eq!(LocalFileErrorKind::AlreadyExists, failure.error().kind());
    assert!(failure.cleanup_error().is_some());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().bytes());
    assert_eq!(0, failure.partial_stats().skipped());
    assert_eq!(0, failure.partial_stats().overwritten());
    assert_eq!(SENTINEL, fs::read(directory.path().join("target")).unwrap().as_slice());
    assert!(failure.staging_path().is_some());
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.

//    SPDX-License-Identifier: Apache-2.0

//    Licensed under the Apache License, Version 2.0.
// =============================================================================
