// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    io::{
        Read,
        Write,
    },
    path::Path,
    time::Duration,
};

use qubit_local_files::{
    LocalAtomicityRequirement,
    LocalCopyMethod,
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalDurabilityRequirement,
    LocalFileErrorKind,
    LocalFileKind,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
    RootedLocalFileSystem,
};
use tempfile::tempdir;

/// Runs a coverage-only fault case in an isolated child test process.
///
/// The child receives one fault selector while the parent stays free of the
/// process-global selector used by concurrent coverage tests.
#[cfg(coverage)]
fn run_in_coverage_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if std::env::var_os(COVERAGE_FAULT_ENV).is_some() {
        action();
        return;
    }
    let executable =
        std::env::current_exe().expect("current test executable should exist");
    let selected_test = if executable
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("lib_tests-"))
    {
        format!("rooted_local_file_system_coverage_tests::{test_name}")
    } else {
        test_name.to_owned()
    };
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(selected_test)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child should launch");
    assert!(status.success(), "coverage fault child should pass");
}

/// Verifies a native temporary-file name collision is retried until the
/// caller-configured attempt budget is exhausted.
#[cfg(coverage)]
#[test]
fn test_rooted_temp_file_reports_injected_name_collision_exhaustion() {
    const TEST_NAME: &str =
        "test_rooted_temp_file_reports_injected_name_collision_exhaustion";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-temp-file-collision",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            let rooted = RootedLocalFileSystem::open(directory.path())
                .expect("root authority should open");

            let error = rooted
                .create_temp_file(
                    &LocalTempFileOptions::new().with_max_attempts(1),
                )
                .expect_err("an exhausted rooted collision budget must fail");

            assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
        },
    );
}

/// Verifies a native temporary-file creation error is reported with the
/// operation's native error kind instead of being retried as a collision.
#[cfg(coverage)]
#[test]
fn test_rooted_temp_file_reports_injected_native_creation_error() {
    const TEST_NAME: &str =
        "test_rooted_temp_file_reports_injected_native_creation_error";
    run_in_coverage_fault_process(TEST_NAME, "rooted-temp-file-open", || {
        let directory =
            tempdir().expect("temporary directory should be created");
        let rooted = RootedLocalFileSystem::open(directory.path())
            .expect("root authority should open");

        let error = rooted
            .create_temp_file(&LocalTempFileOptions::new().with_max_attempts(1))
            .expect_err("a rooted native file creation failure must surface");

        assert_eq!(LocalFileErrorKind::PermissionDenied, error.kind());
    });
}

/// Verifies a native temporary-directory creation failure retains its rooted
/// operation context instead of being treated as a name collision.
#[cfg(coverage)]
#[test]
fn test_rooted_temp_directory_reports_injected_native_creation_error() {
    const TEST_NAME: &str =
        "test_rooted_temp_directory_reports_injected_native_creation_error";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-temp-directory-create",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            let rooted = RootedLocalFileSystem::open(directory.path())
                .expect("root authority should open");

            let error = rooted
                .create_temp_directory(
                    &LocalTempDirectoryOptions::new().with_max_attempts(1),
                )
                .expect_err(
                    "a rooted native directory creation failure must surface",
                );

            assert_eq!(LocalFileErrorKind::PermissionDenied, error.kind());
        },
    );
}

/// Verifies a native temporary-directory name collision is retried until the
/// caller-configured attempt budget is exhausted.
#[cfg(coverage)]
#[test]
fn test_rooted_temp_directory_reports_injected_name_collision_exhaustion() {
    const TEST_NAME: &str =
        "test_rooted_temp_directory_reports_injected_name_collision_exhaustion";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-temp-directory-collision",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            let rooted = RootedLocalFileSystem::open(directory.path())
                .expect("root authority should open");

            let error = rooted
                .create_temp_directory(
                    &LocalTempDirectoryOptions::new().with_max_attempts(1),
                )
                .expect_err("an exhausted rooted collision budget must fail");

            assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
        },
    );
}

/// Verifies a rooted directory status-read failure is preserved at the facade
/// boundary before it can be mistaken for an absent entry.
#[cfg(coverage)]
#[test]
fn test_rooted_directory_creation_reports_injected_status_error() {
    const TEST_NAME: &str =
        "test_rooted_directory_creation_reports_injected_status_error";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-local-create-directory-status",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            let rooted = RootedLocalFileSystem::open(directory.path())
                .expect("root authority should open");

            let error = rooted
                .create_directory(
                    Path::new("entry"),
                    &LocalCreateDirectoryOptions::new(),
                )
                .expect_err("an injected rooted status read must fail");

            assert_eq!(LocalFileErrorKind::PermissionDenied, error.kind());
            assert_eq!(Some(Path::new("entry")), error.path());
        },
    );
}

/// Verifies rooted accessors expose the opened diagnostic anchor and the
/// platform capability snapshot used by rooted operations.
#[test]
fn test_rooted_local_file_system_exposes_opened_anchor_and_capabilities() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    assert_eq!(directory.path(), rooted.diagnostic_path());
    assert_eq!(
        rooted.capabilities().rooted_operations_implemented(),
        RootedLocalFileSystem::open(directory.path())
            .expect("second root authority should open")
            .capabilities()
            .rooted_operations_implemented(),
    );
}

/// Verifies opening a regular file as a rooted authority is rejected at the
/// native-root boundary with the original diagnostic path.
#[test]
fn test_rooted_local_file_system_rejects_regular_file_anchor() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("not-a-directory");
    fs::write(&file, b"payload")
        .expect("regular-file fixture should be written");

    let error = RootedLocalFileSystem::open(&file)
        .expect_err("regular files cannot become rooted authorities");

    assert_eq!(LocalFileErrorKind::NotDirectory, error.kind());
    assert_eq!(
        qubit_local_files::LocalFileOperation::OpenRoot,
        error.operation()
    );
    assert_eq!(Some(file.as_path()), error.path());
}

/// Verifies rooted temporary resources support validated descendant parents and
/// preserve invalid-affix failures at the rooted API boundary.
#[test]
fn test_rooted_local_file_system_temp_resources_use_descendant_parent() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("temporary-parent"))
        .expect("temporary parent should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let file = rooted
        .create_temp_file(
            &LocalTempFileOptions::new()
                .with_parent(Path::new("temporary-parent"))
                .with_prefix("file-")
                .with_suffix(".tmp"),
        )
        .expect("rooted temporary file should be created");
    assert!(file.path().starts_with("temporary-parent"));
    let mut file = file;
    file.cleanup()
        .expect("rooted temporary file should clean up");

    let directory_resource = rooted
        .create_temp_directory(
            &LocalTempDirectoryOptions::new()
                .with_parent(Path::new("temporary-parent"))
                .with_prefix("directory-")
                .with_suffix(".tmp"),
        )
        .expect("rooted temporary directory should be created");
    assert!(directory_resource.path().starts_with("temporary-parent"));
    let mut directory_resource = directory_resource;
    directory_resource
        .cleanup()
        .expect("rooted temporary directory should clean up");

    let error = rooted
        .create_temp_file(&LocalTempFileOptions::new().with_prefix("bad/name"))
        .expect_err("path separator in temporary prefix must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());

    let retry_error = rooted
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_max_attempts(0),
        )
        .expect_err("zero rooted directory retry budget must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidOptions, retry_error.kind());
}

/// Verifies rooted deletion and rename APIs cover successful, accepted-missing,
/// conflict, and replacement paths while remaining descriptor-relative.
#[test]
fn test_rooted_local_file_system_deletes_and_renames_entries() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    fs::write(directory.path().join("source"), b"source")
        .expect("source fixture should be written");
    fs::write(directory.path().join("target"), b"target")
        .expect("target fixture should be written");

    let conflict = rooted
        .rename(
            Path::new("source"),
            Path::new("target"),
            &LocalRenameOptions::new(),
        )
        .expect_err(
            "rooted default rename must not replace an existing target",
        );
    assert_eq!(LocalFileErrorKind::AlreadyExists, conflict.error().kind());
    let _ = rooted
        .rename(
            Path::new("source"),
            Path::new("target"),
            &LocalRenameOptions::new().with_overwrite(),
        )
        .expect("rooted overwrite rename should succeed");
    assert_eq!(
        b"source",
        fs::read(directory.path().join("target"))
            .expect("replacement target should be readable")
            .as_slice(),
    );

    assert!(
        rooted
            .delete_file(Path::new("target"), &LocalDeleteOptions::new())
            .expect("rooted file should delete")
            .deleted()
    );
    assert!(
        !rooted
            .delete_file(
                Path::new("missing"),
                &LocalDeleteOptions::new().with_missing_ok(),
            )
            .expect("missing rooted file should be accepted")
            .deleted()
    );

    let _ = rooted
        .create_directory(
            Path::new("tree/child"),
            &LocalCreateDirectoryOptions::new().with_recursive(),
        )
        .expect("rooted directory tree should be created");
    assert!(
        rooted
            .delete_directory(
                Path::new("tree"),
                &LocalDeleteOptions::new().with_recursive(),
            )
            .expect("rooted directory tree should delete")
            .deleted()
    );
}

/// Verifies rooted walkers expose their diagnostic root and staged writers
/// accept vectored bytes before publication.
#[test]
fn test_rooted_local_file_system_walker_and_staged_writer_cover_accessors() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("listing"))
        .expect("listing directory should be created");
    fs::write(directory.path().join("listing/entry"), b"entry")
        .expect("listing entry should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let walker = rooted
        .list(Path::new("listing"), &LocalListOptions::new())
        .expect("rooted walker should open");
    assert_eq!(directory.path().join("listing"), walker.root());
    assert_eq!(
        1,
        walker
            .collect::<Result<Vec<_>, _>>()
            .expect("rooted walker should yield entry")
            .len(),
    );

    let mut writer = rooted
        .open_writer(
            Path::new("written"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted staged writer should open");
    assert_eq!(
        6,
        writer
            .write_vectored(&[
                std::io::IoSlice::new(b"vec"),
                std::io::IoSlice::new(b"tor"),
            ])
            .expect("rooted staged writer should accept vectored bytes")
    );
    let _ = writer.commit().expect("rooted staged writer should commit");
    assert_eq!(
        b"vector",
        fs::read(directory.path().join("written"))
            .expect("published rooted payload should read")
            .as_slice(),
    );
}

/// Verifies aborting a rooted staged writer removes staging without publishing
/// the bound destination.
#[test]
fn test_rooted_local_file_system_writer_abort_discards_staging() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut writer = rooted
        .open_writer(
            Path::new("discarded"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted staged writer should open");
    writer
        .write_all(b"staged")
        .expect("rooted writer should accept staged bytes");

    let outcome = writer.abort().expect("rooted writer should abort");

    assert!(!directory.path().join("discarded").exists());
    assert_eq!(
        qubit_local_files::LocalWriterState::Aborted,
        outcome.state()
    );
}

/// Verifies rooted metadata, reader, and append writer operations retain
/// descriptor-relative authority and reject missing reader paths precisely.
#[test]
fn test_rooted_local_file_system_reads_metadata_and_appends() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("payload"), b"base")
        .expect("payload fixture should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let metadata = rooted
        .metadata(Path::new("payload"))
        .expect("rooted metadata should be available");
    assert_eq!(LocalFileKind::File, metadata.kind());
    let mut reader = rooted
        .open_reader(Path::new("payload"), &LocalReadOptions::new())
        .expect("rooted reader should open");
    let mut payload = String::new();
    reader
        .read_to_string(&mut payload)
        .expect("rooted reader should read payload");
    assert_eq!("base", payload);

    let mut writer = rooted
        .open_writer(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect("rooted append writer should open");
    writer
        .write_all(b"-append")
        .expect("rooted append writer should accept bytes");
    let _ = writer.commit().expect("rooted append writer should commit");
    assert_eq!(
        b"base-append",
        fs::read(directory.path().join("payload"))
            .expect("appended rooted payload should read")
            .as_slice(),
    );

    let missing = rooted
        .open_reader(Path::new("missing"), &LocalReadOptions::new())
        .expect_err("missing rooted reader path must fail");
    assert_eq!(LocalFileErrorKind::NotFound, missing.kind());
}

/// Verifies rooted create, list, delete, reader, and writer policy failures
/// preserve their operation-specific classification before native mutation.
#[test]
fn test_rooted_local_file_system_rejects_incompatible_entry_policies() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("file"), b"payload")
        .expect("file fixture should be written");
    fs::create_dir(directory.path().join("directory"))
        .expect("directory fixture should be created");
    fs::write(directory.path().join("directory/child"), b"child")
        .expect("directory child should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let create_error = rooted
        .create_directory(
            Path::new("file"),
            &LocalCreateDirectoryOptions::new().with_exists_ok(),
        )
        .expect_err("regular file cannot satisfy rooted directory creation");
    assert_eq!(LocalFileErrorKind::TypeConflict, create_error.kind());
    let reader_error = rooted
        .open_reader(Path::new("directory"), &LocalReadOptions::new())
        .expect_err("rooted directories must not open as readers");
    assert_eq!(LocalFileErrorKind::TypeConflict, reader_error.kind());
    let writer_error = rooted
        .open_writer(
            Path::new("file"),
            &LocalWriteOptions::new(LocalWriteMode::Append)
                .with_atomicity(LocalAtomicityRequirement::Required),
        )
        .expect_err("rooted append cannot promise required atomicity");
    assert_eq!(LocalFileErrorKind::RequirementNotMet, writer_error.kind());

    let delete_file_error = rooted
        .delete_file(Path::new("directory"), &LocalDeleteOptions::new())
        .expect_err("rooted directories cannot be deleted as files");
    assert_eq!(LocalFileErrorKind::IsDirectory, delete_file_error.kind());
    let delete_directory_error = rooted
        .delete_directory(Path::new("file"), &LocalDeleteOptions::new())
        .expect_err("rooted files cannot be deleted as directories");
    assert_eq!(LocalFileErrorKind::NotDirectory, delete_directory_error.kind());
    let non_recursive_error = rooted
        .delete_directory(Path::new("directory"), &LocalDeleteOptions::new())
        .expect_err("non-empty rooted directory requires recursive deletion");
    assert_eq!(LocalFileErrorKind::Io, non_recursive_error.kind());
}

/// Verifies a concurrent rooted create-new target remains intact and a failed
/// staged commit reports the proven not-published state.
#[test]
fn test_rooted_local_file_system_create_new_commit_preserves_concurrent_target()
{
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut writer = rooted
        .open_writer(
            Path::new("target"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted create-new writer should open");
    writer
        .write_all(b"staged")
        .expect("rooted staged writer should accept bytes");
    fs::write(directory.path().join("target"), b"concurrent")
        .expect("concurrent rooted target should be created");

    let error = writer.commit().expect_err(
        "rooted create-new commit must not replace concurrent target",
    );

    assert_eq!(
        qubit_local_files::LocalWriteFailureState::NotPublished,
        error.state()
    );
    assert_eq!(
        b"concurrent",
        fs::read(directory.path().join("target"))
            .expect("concurrent target should remain readable")
            .as_slice(),
    );
}

/// Verifies rooted directory, file-copy, and direct-writer branches retain
/// their distinct conflict and parent-creation policies.
#[test]
fn test_rooted_local_file_system_exercises_directory_copy_and_writer_policies()
{
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let created = rooted
        .create_directory(
            Path::new("directory"),
            &LocalCreateDirectoryOptions::new(),
        )
        .expect("new rooted directory should be created");
    assert!(created.created());
    let conflict = rooted
        .create_directory(
            Path::new("directory"),
            &LocalCreateDirectoryOptions::new(),
        )
        .expect_err(
            "existing rooted directory should require explicit acceptance",
        );
    assert_eq!(LocalFileErrorKind::AlreadyExists, conflict.kind());
    let missing_parent = rooted
        .create_directory(
            Path::new("missing/child"),
            &LocalCreateDirectoryOptions::new(),
        )
        .expect_err(
            "non-recursive rooted creation must retain missing-parent errors",
        );
    assert_eq!(LocalFileErrorKind::NotFound, missing_parent.kind());

    fs::write(directory.path().join("source"), b"payload")
        .expect("rooted copy source should be written");
    let copied = rooted
        .copy(
            Path::new("source"),
            Path::new("copied"),
            &LocalCopyOptions::new(),
        )
        .expect("rooted regular-file copy should succeed");
    assert_eq!(LocalCopyMethod::StagedFile, copied.method());
    assert!(copied.atomic());
    assert_eq!(
        b"payload",
        fs::read(directory.path().join("copied"))
            .expect("copied rooted file should be readable")
            .as_slice(),
    );
    let missing_copy = rooted
        .copy(
            Path::new("missing"),
            Path::new("unwritten"),
            &LocalCopyOptions::new(),
        )
        .expect_err("missing rooted copy source must not publish a target");
    assert_eq!(LocalFileErrorKind::NotFound, missing_copy.error().kind());
    assert!(!directory.path().join("unwritten").exists());

    let mut writer = rooted
        .open_writer(
            Path::new("created-parent/payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_parent(),
        )
        .expect("rooted writer should create requested parents");
    writer
        .write_all(b"created")
        .expect("rooted parent-creating writer should accept bytes");
    let outcome = writer
        .commit()
        .expect("rooted parent-creating writer should commit");
    assert!(outcome.atomic());
    assert_eq!(
        b"created",
        fs::read(directory.path().join("created-parent/payload"))
            .expect("parent-created rooted payload should be readable")
            .as_slice(),
    );
    let append_directory = rooted
        .open_writer(
            Path::new("directory"),
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect_err("rooted append writer must reject directories");
    assert_eq!(LocalFileErrorKind::TypeConflict, append_directory.kind());
}

/// Verifies rooted copy and rename distinguish preferred from required parent
/// durability without weakening a platform's advertised capability contract.
#[test]
fn test_rooted_local_file_system_copy_and_rename_report_durability() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let supports_durability =
        rooted.capabilities().directory_durability_implemented();

    fs::write(directory.path().join("copy-source"), b"copy")
        .expect("rooted copy source should be written");
    let preferred_copy = rooted
        .copy(
            Path::new("copy-source"),
            Path::new("copy-preferred"),
            &LocalCopyOptions::new()
                .with_durability(LocalDurabilityRequirement::Preferred),
        )
        .expect("preferred rooted copy should publish its destination");
    assert_eq!(LocalCopyMethod::StagedFile, preferred_copy.method());

    fs::write(directory.path().join("copy-required-source"), b"required")
        .expect("required rooted copy source should be written");
    let required_copy = rooted.copy(
        Path::new("copy-required-source"),
        Path::new("copy-required"),
        &LocalCopyOptions::new()
            .with_durability(LocalDurabilityRequirement::Required),
    );
    if supports_durability {
        assert!(
            required_copy
                .expect("advertised rooted copy durability should be achieved")
                .durable()
        );
    } else {
        assert_eq!(
            LocalFileErrorKind::RequirementNotMet,
            required_copy
                .expect_err("unsupported rooted copy durability must fail before publication")
                .error()
                .kind(),
        );
    }

    fs::write(directory.path().join("rename-preferred-source"), b"rename")
        .expect("preferred rooted rename source should be written");
    let preferred_rename = rooted
        .rename(
            Path::new("rename-preferred-source"),
            Path::new("rename-preferred"),
            &LocalRenameOptions::new()
                .with_durability(LocalDurabilityRequirement::Preferred),
        )
        .expect("preferred rooted rename should publish its destination");
    assert!(preferred_rename.atomic());

    fs::write(directory.path().join("rename-required-source"), b"required")
        .expect("required rooted rename source should be written");
    let required_rename = rooted.rename(
        Path::new("rename-required-source"),
        Path::new("rename-required"),
        &LocalRenameOptions::new()
            .with_durability(LocalDurabilityRequirement::Required),
    );
    if supports_durability {
        assert!(
            required_rename
                .expect(
                    "advertised rooted rename durability should be achieved"
                )
                .durable()
        );
    } else {
        assert_eq!(
            LocalFileErrorKind::RequirementNotMet,
            required_rename
                .expect_err("unsupported rooted rename durability must fail before publication")
                .error()
                .kind(),
        );
    }
}

/// Verifies rooted staged replacement keeps a cleanup-capable writer when the
/// inspected destination disappears before publication begins.
#[test]
fn test_rooted_local_file_system_returns_retryable_writer_before_publication() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("payload"), b"existing")
        .expect("existing rooted payload should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut writer = rooted
        .open_writer(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("rooted replacement writer should open");
    writer
        .write_all(b"staged")
        .expect("rooted replacement writer should accept bytes");
    fs::remove_file(directory.path().join("payload"))
        .expect("inspected rooted destination should be removed before commit");

    let error = writer
        .commit()
        .expect_err("missing rooted destination should prevent publication");
    assert_eq!(
        qubit_local_files::LocalWriteFailureState::NotPublished,
        error.state()
    );
    let (_cause, state, retryable) = error.into_parts();
    assert_eq!(
        qubit_local_files::LocalWriteFailureState::NotPublished,
        state,
    );
    let outcome = retryable
        .expect("rooted pre-publication failure should retain staging")
        .abort()
        .expect("retained rooted staging should clean up");
    assert_eq!(
        qubit_local_files::LocalWriterState::Aborted,
        outcome.state()
    );
    assert!(!directory.path().join("payload").exists());
}

/// Verifies configured zero retry deadlines still permit the first rooted
/// reader and writer open attempt when no conflicting lease exists.
#[test]
fn test_rooted_local_file_system_opens_reader_and_writer_with_retry_deadlines()
{
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("reader"), b"reader")
        .expect("rooted reader fixture should be written");
    fs::write(directory.path().join("append"), b"append")
        .expect("rooted append fixture should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let mut reader = rooted
        .open_reader(
            Path::new("reader"),
            &LocalReadOptions::new().with_open_retry_timeout(Duration::ZERO),
        )
        .expect("initial rooted reader attempt should not need a retry");
    let mut contents = String::new();
    reader
        .read_to_string(&mut contents)
        .expect("rooted reader should read its fixture");
    assert_eq!("reader", contents);

    let mut staged = rooted
        .open_writer(
            Path::new("staged"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew)
                .with_open_retry_timeout(Duration::ZERO),
        )
        .expect("initial rooted staged writer attempt should not need a retry");
    staged
        .write_all(b"staged")
        .expect("rooted staged writer should accept bytes");
    let _ = staged.commit().expect("rooted staged writer should commit");

    let mut append = rooted
        .open_writer(
            Path::new("append"),
            &LocalWriteOptions::new(LocalWriteMode::Append)
                .with_open_retry_timeout(Duration::ZERO),
        )
        .expect("initial rooted append writer attempt should not need a retry");
    append
        .write_all(b"+")
        .expect("rooted append writer should accept bytes");
    let _ = append.commit().expect("rooted append writer should commit");
    assert_eq!(
        b"append+",
        fs::read(directory.path().join("append"))
            .expect("rooted append result should be readable")
            .as_slice(),
    );
}

/// Verifies rooted metadata and strict deletion preserve not-found context
/// when callers do not opt into accepting missing entries.
#[test]
fn test_rooted_local_file_system_reports_strict_missing_entry_errors() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let metadata = rooted
        .metadata(Path::new("missing-metadata"))
        .expect_err("missing rooted metadata must retain its failure");
    assert_eq!(LocalFileErrorKind::NotFound, metadata.kind());
    assert_eq!(Some(Path::new("missing-metadata")), metadata.path());

    let file = rooted
        .delete_file(Path::new("missing-file"), &LocalDeleteOptions::new())
        .expect_err("strict rooted file deletion must reject a missing entry");
    assert_eq!(LocalFileErrorKind::NotFound, file.kind());
    assert_eq!(Some(Path::new("missing-file")), file.path());

    let directory = rooted
        .delete_directory(
            Path::new("missing-directory"),
            &LocalDeleteOptions::new(),
        )
        .expect_err(
            "strict rooted directory deletion must reject a missing entry",
        );
    assert_eq!(LocalFileErrorKind::NotFound, directory.kind());
    assert_eq!(Some(Path::new("missing-directory")), directory.path());
}

/// Verifies rooted staging cleanup preserves its abort error when a concurrent
/// actor removes the descriptor-relative temporary file before cleanup.
#[test]
fn test_rooted_local_file_system_abort_reports_missing_staging_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut writer = rooted
        .open_writer(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted staged writer should open");
    let staging = fs::read_dir(directory.path())
        .expect("root staging directory should be readable")
        .map(|entry| {
            entry.expect("root staging entry should be readable").path()
        })
        .find(|path| path != &target)
        .expect("rooted writer should create one temporary file");
    fs::remove_file(&staging)
        .expect("concurrent actor should remove rooted staging file");

    let error = writer
        .abort()
        .expect_err("missing rooted staging file must report cleanup failure");
    assert_eq!(
        qubit_local_files::LocalFileOperation::Abort,
        error.operation()
    );
    assert_eq!(Some(target.as_path()), error.path());
    assert!(!target.exists());
}

/// Verifies rooted preflight rejects invalid rename operands and writer paths
/// before any namespace mutation or staging publication can occur.
#[test]
fn test_rooted_local_file_system_rejects_invalid_preflight_operands() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("source"), b"source")
        .expect("rooted rename source should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let rename = rooted
        .rename(
            Path::new("../escape"),
            Path::new("target"),
            &LocalRenameOptions::new(),
        )
        .expect_err("rooted rename must reject lexical source escape");
    assert_eq!(LocalFileErrorKind::InvalidPath, rename.error().kind());
    assert_eq!(
        qubit_local_files::LocalRenameFailureState::Unchanged,
        rename.state(),
    );
    assert!(directory.path().join("source").exists());

    let staged = rooted
        .open_writer(
            Path::new("missing-parent/payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err(
            "rooted writer without parent creation must reject missing parents",
        );
    assert_eq!(LocalFileErrorKind::NotFound, staged.kind());
    let append = rooted
        .open_writer(
            Path::new("missing-append"),
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect_err("rooted append must inspect an existing regular file");
    assert_eq!(LocalFileErrorKind::NotFound, append.kind());
}

/// Verifies a rooted recursive walker reports a descendant that disappears
/// after the root directory was enumerated but before it can be opened.
#[test]
fn test_rooted_local_file_system_walker_reports_disappearing_child_directory() {
    let directory = tempdir().expect("temporary directory should be created");
    let child = directory.path().join("child");
    fs::create_dir(&child).expect("rooted child directory should be created");
    fs::write(child.join("entry"), b"payload")
        .expect("rooted child fixture should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut walker = rooted
        .list(Path::new(""), &LocalListOptions::new().with_recursive())
        .expect("rooted walker should enumerate its root first");
    let entry = walker
        .next()
        .expect("rooted child should be yielded before descent")
        .expect("rooted child metadata should be readable");
    assert_eq!(Path::new("child"), entry.relative_path());
    fs::remove_dir_all(&child)
        .expect("concurrent actor should remove child before descent");

    let error = walker
        .next()
        .expect("deferred descendant opening should report an error")
        .expect_err("disappearing rooted child should fail on descent");
    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
    assert_eq!(Some(Path::new("child")), error.path());
}

/// Verifies rooted append maps a native regular-file open failure after the
/// entry kind was validated through the opened root authority.
#[cfg(unix)]
#[test]
fn test_rooted_local_file_system_append_reports_unwritable_regular_file() {
    use std::os::unix::fs::PermissionsExt;

    // SAFETY: `geteuid` reads the current process identity without pointers or
    // mutable state.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("payload");
    fs::write(&target, b"payload")
        .expect("rooted append fixture should be written");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o400))
        .expect("rooted append fixture should become read-only");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let error = rooted
        .open_writer(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect_err("rooted append must report an unwritable regular file");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
        .expect("rooted append fixture permissions should be restored");

    assert_eq!(LocalFileErrorKind::PermissionDenied, error.kind());
    assert_eq!(Some(Path::new("payload")), error.path());
}

/// Verifies rooted temporary resources retain the configured missing parent in
/// their native creation failures after random names have been generated.
#[test]
fn test_rooted_local_file_system_temp_resources_report_missing_parent() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let file = rooted
        .create_temp_file(
            &LocalTempFileOptions::new().with_parent(Path::new("missing")),
        )
        .expect_err("rooted temporary file must not create an absent parent");
    assert_eq!(LocalFileErrorKind::NotFound, file.kind());
    assert!(
        file.path()
            .expect("temporary file failure should retain candidate path")
            .starts_with("missing")
    );

    let directory = rooted
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(Path::new("missing")),
        )
        .expect_err(
            "rooted temporary directory must not create an absent parent",
        );
    assert_eq!(LocalFileErrorKind::NotFound, directory.kind());
    assert!(
        directory
            .path()
            .expect("temporary directory failure should retain candidate path")
            .starts_with("missing")
    );
}

/// Verifies rooted APIs validate generated name affixes and target descendants
/// independently, preserving the operation-specific preflight failure state.
#[test]
fn test_rooted_local_file_system_rejects_invalid_generated_names_and_targets() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("source"), b"source")
        .expect("rooted copy and rename source should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let temporary_file = rooted
        .create_temp_file(&LocalTempFileOptions::new().with_suffix("/invalid"))
        .expect_err(
            "rooted temporary file suffix must be a single name fragment",
        );
    assert_eq!(LocalFileErrorKind::InvalidOptions, temporary_file.kind());
    let temporary_directory = rooted
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_prefix("invalid/"),
        )
        .expect_err(
            "rooted temporary directory prefix must be a single name fragment",
        );
    assert_eq!(
        LocalFileErrorKind::InvalidOptions,
        temporary_directory.kind(),
    );

    let list = rooted
        .list(Path::new("../escape"), &LocalListOptions::new())
        .expect_err("rooted list must reject lexical escape paths");
    assert_eq!(LocalFileErrorKind::InvalidPath, list.kind());
    let writer = rooted
        .open_writer(
            Path::new("../escape"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("rooted writer must reject lexical escape paths");
    assert_eq!(LocalFileErrorKind::InvalidPath, writer.kind());

    let copy = rooted
        .copy(
            Path::new("source"),
            Path::new("../escape"),
            &LocalCopyOptions::new(),
        )
        .expect_err("rooted copy must reject lexical target escape");
    assert_eq!(LocalFileErrorKind::InvalidPath, copy.error().kind());
    assert_eq!(
        qubit_local_files::LocalCopyFailureState::Unchanged,
        copy.state(),
    );
    let rename = rooted
        .rename(
            Path::new("source"),
            Path::new("../escape"),
            &LocalRenameOptions::new(),
        )
        .expect_err("rooted rename must reject lexical target escape");
    assert_eq!(LocalFileErrorKind::InvalidPath, rename.error().kind());
    assert_eq!(
        qubit_local_files::LocalRenameFailureState::Unchanged,
        rename.state(),
    );
}

/// Runs one coverage-only rooted writer fault in an isolated child process.
#[cfg(all(coverage, target_os = "linux"))]
fn run_rooted_writer_fault<F>(test_name: &str, fault: &str, action: F)
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

/// Verifies a rooted staging installation failure remains not-published and
/// reports that native installation cleanup consumed the staging writer.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_rooted_local_file_system_writer_reports_injected_install_failure() {
    const TEST_NAME: &str =
        "test_rooted_local_file_system_writer_reports_injected_install_failure";
    run_rooted_writer_fault(TEST_NAME, "rooted-install", || {
        let directory =
            tempdir().expect("temporary directory should be created");
        fs::write(directory.path().join("payload"), b"existing")
            .expect("rooted destination should be written");
        let rooted = RootedLocalFileSystem::open(directory.path())
            .expect("root authority should open");
        let mut writer = rooted
            .open_writer(
                Path::new("payload"),
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .expect("rooted replacement writer should open");
        writer
            .write_all(b"replacement")
            .expect("rooted replacement writer should accept bytes");

        let error = writer.commit().expect_err(
            "injected rooted installation failure must fail commit",
        );
        assert_eq!(
            qubit_local_files::LocalWriteFailureState::NotPublished,
            error.state()
        );
        let (_cause, _state, retained) = error.into_parts();
        assert!(retained.is_none());
    });
}
