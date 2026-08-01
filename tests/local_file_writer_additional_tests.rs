// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    error::Error as StdError,
    fs,
    io::{
        IoSlice,
        Write,
    },
};

use qubit_local_files::{
    LocalDurabilityRequirement,
    LocalFileErrorKind,
    LocalFileSystem,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
    LocalWriteFailureState,
};
use tempfile::tempdir;

/// Verifies append commit flushes accepted vectored bytes and reports the
/// direct-publication outcome.
#[test]
fn test_local_file_writer_append_commit_reports_direct_publication() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    fs::write(&target, b"base").expect("payload fixture should be written");
    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::Append),
    )
    .expect("append writer should open");

    assert_eq!(target, writer.diagnostic_path());
    assert_eq!(LocalWriterState::Open, writer.state());
    assert_eq!(
        7,
        writer
            .write_vectored(&[IoSlice::new(b"-vec"), IoSlice::new(b"tor")])
            .expect("append writer should accept vectored bytes")
    );
    writer
        .flush()
        .expect("append writer should flush directly published bytes");
    let outcome = writer.commit().expect("append writer should commit");

    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert!(!outcome.atomic());
    assert!(!outcome.durable());
    assert_eq!(7, outcome.bytes_written());
    assert_eq!(
        b"base-vector",
        fs::read(&target)
            .expect("appended payload should read")
            .as_slice(),
    );
}

/// Verifies append abort distinguishes an untouched session from bytes that
/// have already been directly published.
#[test]
fn test_local_file_writer_append_abort_reports_aborted_and_published_states() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    fs::write(&target, b"base").expect("payload fixture should be written");

    let untouched = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::Append),
    )
    .expect("append writer should open");
    let untouched_outcome =
        untouched.abort().expect("untouched append should abort");
    assert_eq!(LocalWriterState::Aborted, untouched_outcome.state());

    let mut published = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::Append),
    )
    .expect("second append writer should open");
    published
        .write_all(b"-published")
        .expect("append writer should accept bytes");
    let published_outcome =
        published.abort().expect("append abort should flush");
    assert_eq!(LocalWriterState::Aborted, published_outcome.state());
    assert_eq!(
        Some(LocalWriteFailureState::Published),
        published_outcome.failure_state(),
    );
    assert_eq!(
        b"base-published",
        fs::read(&target)
            .expect("published payload should read")
            .as_slice(),
    );
}

/// Verifies staged writers create missing parent directories only when that
/// policy is explicitly requested.
#[test]
fn test_local_file_writer_creates_missing_parent_when_requested() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("nested/payload");
    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateNew).with_parent(),
    )
    .expect("writer should create the requested parent directory");
    writer
        .write_all(b"payload")
        .expect("staged writer should accept bytes");
    let outcome = writer.commit().expect("staged writer should commit");

    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert_eq!(
        b"payload",
        fs::read(target)
            .expect("published payload should read")
            .as_slice(),
    );
}

/// Verifies flushing a staged writer does not publish its destination before a
/// later successful commit.
#[test]
fn test_local_file_writer_flushes_staging_without_publishing_destination() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateNew),
    )
    .expect("staged writer should open");
    writer
        .write_all(b"payload")
        .expect("staged writer should accept bytes");
    writer
        .flush()
        .expect("staged writer should flush its staging file");

    assert!(!target.exists());
    let outcome = writer
        .commit()
        .expect("staged writer should commit after flush");
    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert_eq!(
        b"payload",
        fs::read(target)
            .expect("published payload should read")
            .as_slice(),
    );
}

/// Verifies a concurrent create-new conflict preserves the destination and
/// reports that no retryable writer remains after staging cleanup.
#[test]
fn test_local_file_writer_commit_conflict_preserves_concurrent_destination() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateNew),
    )
    .expect("create-new writer should open for an absent target");
    writer
        .write_all(b"staged")
        .expect("writer should accept staged bytes");
    fs::write(&target, b"concurrent")
        .expect("concurrent destination should be created");

    let error = writer
        .commit()
        .expect_err("commit must preserve a concurrently created target");
    assert_eq!(LocalWriteFailureState::NotPublished, error.state());
    assert!(error.writer().is_none());
    let (_cause, state, retained) = error.into_parts();
    assert_eq!(LocalWriteFailureState::NotPublished, state);
    assert!(retained.is_none());
    assert_eq!(
        b"concurrent",
        fs::read(&target)
            .expect("concurrent target should remain")
            .as_slice(),
    );
}

/// Verifies a public commit failure exposes its source, formatter output, and
/// consumed retry context consistently.
#[test]
fn test_local_file_commit_error_exposes_complete_public_context() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateNew),
    )
    .expect("create-new writer should open before the target exists");
    writer
        .write_all(b"staged")
        .expect("staged writer should accept payload");
    fs::write(&target, b"concurrent")
        .expect("concurrent destination should be created");

    let error = writer
        .commit()
        .expect_err("concurrent destination must fail create-new commit");
    assert_eq!(LocalWriteFailureState::NotPublished, error.state());
    assert_eq!(LocalFileErrorKind::AlreadyExists, error.error().kind());
    assert!(error.writer().is_none());
    assert!(error.to_string().contains("NotPublished"));
    assert!(StdError::source(&error).is_some());

    let (cause, state, retained) = error.into_parts();
    assert_eq!(LocalWriteFailureState::NotPublished, state);
    assert_eq!(LocalFileErrorKind::AlreadyExists, cause.kind());
    assert!(retained.is_none());
}

/// Verifies direct append reports the achieved synchronization guarantee for
/// both optional and required durability policies on a regular file.
#[test]
fn test_local_file_writer_append_honors_durability_policies() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    fs::write(&target, b"base").expect("payload fixture should be written");

    for requirement in [
        LocalDurabilityRequirement::Preferred,
        LocalDurabilityRequirement::Required,
    ] {
        let mut writer = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::Append)
                .with_durability(requirement),
        )
        .expect("append writer should open for a regular file");
        writer
            .write_all(b"+")
            .expect("append writer should accept one byte");
        let outcome = writer
            .commit()
            .expect("regular-file durability synchronization should succeed");

        assert_eq!(LocalWriterState::Committed, outcome.state());
        assert!(outcome.durable());
        assert_eq!(1, outcome.bytes_written());
    }
    assert_eq!(
        b"base++",
        fs::read(target)
            .expect("durably appended payload should read")
            .as_slice(),
    );
}

/// Verifies a staged replacement failure before publication returns a writer
/// that callers can explicitly abort, preserving the original stream count.
#[test]
fn test_local_file_writer_returns_retryable_writer_before_publication() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    fs::write(&target, b"existing")
        .expect("existing payload should be written");
    let mut writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    )
    .expect("replacement writer should open before destination removal");
    writer
        .write_all(b"staged")
        .expect("replacement writer should accept staged bytes");
    fs::remove_file(&target)
        .expect("existing destination should be removed before commit");

    let error = writer
        .commit()
        .expect_err("missing inspected destination should prevent publication");
    assert_eq!(LocalWriteFailureState::NotPublished, error.state());
    let (_cause, state, retryable) = error.into_parts();
    assert_eq!(LocalWriteFailureState::NotPublished, state);
    let outcome = retryable
        .expect("pre-publication failure should retain the staged writer")
        .abort()
        .expect("retained staged writer should clean up");
    assert_eq!(LocalWriterState::Aborted, outcome.state());
    assert_eq!(6, outcome.bytes_written());
    assert!(!target.exists());
}

/// Verifies explicit host staging cleanup preserves its structured error when
/// an external actor removes the temporary file before abort begins.
#[test]
fn test_local_file_writer_abort_reports_missing_host_staging_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    let writer = LocalFileSystem::open_writer(
        &target,
        &LocalWriteOptions::new(LocalWriteMode::CreateNew),
    )
    .expect("staged writer should open");
    let staging = fs::read_dir(directory.path())
        .expect("staging directory should be readable")
        .map(|entry| entry.expect("staging entry should be readable").path())
        .find(|path| path != &target)
        .expect("staged writer should create one temporary file");
    fs::remove_file(&staging)
        .expect("external actor should remove staging file");

    let error = writer.abort().expect_err(
        "missing host staging file must report explicit cleanup failure",
    );
    assert_eq!(
        qubit_local_files::LocalFileOperation::Abort,
        error.operation()
    );
    assert_eq!(Some(target.as_path()), error.path());
    assert!(!target.exists());
}

/// Verifies rooted facade writers preserve the shared writer outcome contract
/// for both commit and explicit abort.
#[cfg(unix)]
#[test]
fn test_local_file_writer_rooted_sessions_report_commit_and_abort_outcomes() {
    use std::path::Path;

    use qubit_local_files::RootedLocalFileSystem;

    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("rooted filesystem should open");

    let mut committed = rooted
        .open_writer(
            Path::new("committed"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted staged writer should open");
    assert_eq!(
        6,
        committed
            .write_vectored(&[IoSlice::new(b"root"), IoSlice::new(b"ed")])
            .expect("rooted writer should accept vectored bytes")
    );
    committed.flush().expect("rooted staging should flush");
    let committed_outcome = committed
        .commit()
        .expect("rooted staged writer should commit");

    assert_eq!(LocalWriterState::Committed, committed_outcome.state());
    assert!(committed_outcome.atomic());
    assert_eq!(6, committed_outcome.bytes_written());
    assert_eq!(
        b"rooted",
        fs::read(directory.path().join("committed"))
            .expect("committed rooted payload should read")
            .as_slice(),
    );

    let mut aborted = rooted
        .open_writer(
            Path::new("aborted"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("second rooted staged writer should open");
    aborted
        .write_all(b"discarded")
        .expect("aborted rooted writer should accept bytes");
    let aborted_outcome = aborted.abort().expect("rooted writer should abort");

    assert_eq!(LocalWriterState::Aborted, aborted_outcome.state());
    assert_eq!(9, aborted_outcome.bytes_written());
    assert!(!directory.path().join("aborted").exists());
}

/// Verifies rooted staged failures retain the facade writer for explicit
/// cleanup when the inspected destination disappears before publication.
#[cfg(unix)]
#[test]
fn test_local_file_writer_rooted_prepublication_failure_retains_writer() {
    use std::path::Path;

    use qubit_local_files::RootedLocalFileSystem;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    fs::write(&target, b"existing")
        .expect("existing rooted payload should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("rooted filesystem should open");
    let mut writer = rooted
        .open_writer(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("rooted replacement writer should open");
    writer
        .write_all(b"staged")
        .expect("rooted replacement should accept staged bytes");
    fs::remove_file(&target)
        .expect("inspected rooted destination should be removed");

    let error = writer
        .commit()
        .expect_err("missing rooted destination must prevent publication");
    assert_eq!(LocalWriteFailureState::NotPublished, error.state());
    let (_cause, state, retained) = error.into_parts();
    assert_eq!(LocalWriteFailureState::NotPublished, state);
    let outcome = retained
        .expect("prepublication rooted failure should retain writer")
        .abort()
        .expect("retained rooted writer should clean staging");
    assert_eq!(LocalWriterState::Aborted, outcome.state());
    assert_eq!(6, outcome.bytes_written());
    assert!(!target.exists());
}

/// Runs one coverage-only host writer fault in an isolated child process.
#[cfg(all(coverage, unix))]
fn run_host_writer_fault<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if std::env::var_os(COVERAGE_FAULT_ENV).is_some() {
        action();
        return;
    }
    let executable = std::env::current_exe()
        .expect("coverage test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage writer fault child should launch");
    assert!(status.success(), "coverage writer fault child should pass");
}

/// Verifies a host staged replacement fault is surfaced as a not-published
/// facade commit failure after native installation cleanup consumes staging.
#[cfg(all(coverage, unix))]
#[test]
fn test_local_file_writer_reports_injected_replacement_failure() {
    const TEST_NAME: &str =
        "test_local_file_writer_reports_injected_replacement_failure";
    run_host_writer_fault(TEST_NAME, "atomic-install-replace", || {
        let directory =
            tempdir().expect("temporary directory should be created");
        let target = directory.path().join("payload");
        fs::write(&target, b"existing")
            .expect("replacement destination should be written");
        let mut writer = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("replacement writer should open");
        writer
            .write_all(b"replacement")
            .expect("replacement writer should accept staged bytes");

        let error = writer
            .commit()
            .expect_err("injected replacement failure must fail commit");
        assert_eq!(LocalWriteFailureState::NotPublished, error.state());
        let (_cause, _state, retained) = error.into_parts();
        assert!(retained.is_none());
        assert_eq!(
            b"existing",
            fs::read(&target)
                .expect("failed host replacement should preserve destination")
                .as_slice(),
        );
    });
}

/// Verifies an injected direct-append flush failure is classified as an
/// indeterminate publication after bytes may have reached the destination.
#[cfg(all(coverage, unix))]
#[test]
fn test_local_file_writer_reports_injected_append_commit_flush_failure() {
    const TEST_NAME: &str =
        "test_local_file_writer_reports_injected_append_commit_flush_failure";
    run_host_writer_fault(TEST_NAME, "writer-append-commit-flush", || {
        let directory = tempdir().expect("temporary directory should exist");
        let target = directory.path().join("payload");
        fs::write(&target, b"base").expect("payload should be written");
        let mut writer = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect("append writer should open");
        writer
            .write_all(b"+")
            .expect("append write should succeed before flush fault");

        let error = writer
            .commit()
            .expect_err("injected append flush must fail commit");
        assert_eq!(LocalWriteFailureState::Indeterminate, error.state());
        assert_eq!(LocalFileErrorKind::Indeterminate, error.error().kind());
    });
}

/// Verifies an injected required append synchronization failure reports a
/// published-but-not-durable destination state.
#[cfg(all(coverage, unix))]
#[test]
fn test_local_file_writer_reports_injected_required_append_sync_failure() {
    const TEST_NAME: &str =
        "test_local_file_writer_reports_injected_required_append_sync_failure";
    run_host_writer_fault(TEST_NAME, "writer-append-required-sync", || {
        let directory = tempdir().expect("temporary directory should exist");
        let target = directory.path().join("payload");
        fs::write(&target, b"base").expect("payload should be written");
        let mut writer = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::Append)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .expect("append writer should open");
        writer
            .write_all(b"+")
            .expect("append write should succeed before sync fault");

        let error = writer
            .commit()
            .expect_err("injected append sync must fail commit");
        assert_eq!(LocalWriteFailureState::Published, error.state());
        assert_eq!(
            LocalFileErrorKind::PublicationIncomplete,
            error.error().kind(),
        );
    });
}

/// Verifies an injected direct-append abort flush failure retains the abort
/// operation context instead of claiming a terminal outcome.
#[cfg(all(coverage, unix))]
#[test]
fn test_local_file_writer_reports_injected_append_abort_flush_failure() {
    const TEST_NAME: &str =
        "test_local_file_writer_reports_injected_append_abort_flush_failure";
    run_host_writer_fault(TEST_NAME, "writer-append-abort-flush", || {
        let directory = tempdir().expect("temporary directory should exist");
        let target = directory.path().join("payload");
        fs::write(&target, b"base").expect("payload should be written");
        let writer = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect("append writer should open");

        let error = writer
            .abort()
            .expect_err("injected append flush must fail abort");
        assert_eq!(
            qubit_local_files::LocalFileOperation::Abort,
            error.operation()
        );
    });
}
