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
};

use qubit_local_files::{
    LocalAtomicityRequirement,
    LocalCopyMethod,
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalFileErrorKind,
    LocalFileKind,
    LocalListOptions,
    LocalReadOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
    RootedLocalFileSystem,
};
use tempfile::tempdir;

/// Verifies an opened rooted authority supports a complete create, write, read,
/// list, and recursive-delete workflow without host-path authority.
#[test]
fn test_rooted_local_file_system_runs_core_entry_workflow() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");
    let create_options = LocalCreateDirectoryOptions::new().with_recursive();

    let created = rooted
        .create_directory(Path::new("nested/child"), &create_options)
        .expect("nested rooted directory should be created");
    assert!(created.created());
    let existing = rooted
        .create_directory(
            Path::new("nested/child"),
            &LocalCreateDirectoryOptions::new().with_exists_ok(),
        )
        .expect("existing rooted directory should be accepted explicitly");
    assert!(!existing.created());

    let payload = Path::new("nested/child/payload");
    let mut writer = rooted
        .open_writer(
            payload,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted writer should open");
    assert_eq!(parent.path().join(payload), writer.diagnostic_path());
    writer
        .write_all(b"first")
        .expect("rooted writer should accept staged bytes");
    let outcome = writer.commit().expect("rooted writer should commit");
    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert!(outcome.atomic());

    let metadata = rooted
        .metadata(payload)
        .expect("rooted payload metadata should be available");
    assert_eq!(LocalFileKind::File, metadata.kind());
    let mut reader = rooted
        .open_reader(payload, &LocalReadOptions::new())
        .expect("rooted payload should open for reading");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("rooted reader should return payload bytes");
    assert_eq!("first", content);

    let entries = rooted
        .list(
            Path::new("nested"),
            &LocalListOptions::new().with_recursive(),
        )
        .expect("rooted directory should be listable")
        .collect::<Result<Vec<_>, _>>()
        .expect("rooted traversal should succeed");
    assert_eq!(2, entries.len());

    let deleted = rooted
        .delete_directory(
            Path::new("nested"),
            &LocalDeleteOptions::new().with_recursive(),
        )
        .expect("recursive rooted deletion should succeed");
    assert!(deleted.deleted());
    assert!(!parent.path().join("nested").exists());
}

/// Verifies rooted walkers and writers expose captured diagnostic paths after
/// their opened root has moved, while descriptor-relative operations retain
/// the original authority.
#[cfg(unix)]
#[test]
fn test_rooted_sessions_report_diagnostic_paths_after_root_rename() {
    let parent = tempdir().expect("temporary parent should be created");
    let original = parent.path().join("original");
    let renamed = parent.path().join("renamed");
    fs::create_dir(&original).expect("original root should be created");
    fs::write(original.join("listed"), b"authoritative")
        .expect("listed fixture should be written");
    let rooted = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");
    fs::rename(&original, &renamed).expect("opened root should be renamed");
    fs::create_dir(&original).expect("replacement root should be created");
    fs::create_dir(original.join("listed"))
        .expect("replacement entry should differ in type");

    let entry = rooted
        .list(Path::new(""), &LocalListOptions::new())
        .expect("rooted listing should start")
        .next()
        .expect("opened root should still contain listed entry")
        .expect("rooted listing should succeed");
    assert_eq!(Path::new("listed"), entry.relative_path());
    assert_eq!(original.join("listed"), entry.diagnostic_path());
    assert_eq!(LocalFileKind::File, entry.metadata().kind());

    let mut writer = rooted
        .open_writer(
            Path::new("written"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted writer should open through retained authority");
    assert_eq!(original.join("written"), writer.diagnostic_path());
    writer
        .write_all(b"authoritative")
        .expect("writer should retain opened root authority");
    let _ = writer
        .commit()
        .expect("writer should publish through opened root");
    assert_eq!(
        b"authoritative",
        fs::read(renamed.join("written"))
            .expect("renamed root should receive writer output")
            .as_slice(),
    );
    assert!(!original.join("written").exists());
}

/// Verifies rooted file operations report expected errors and missing-entry
/// policies without escaping the opened authority.
#[test]
fn test_rooted_local_file_system_handles_entry_type_and_missing_policies() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");
    fs::create_dir(parent.path().join("directory"))
        .expect("directory fixture should be created");
    fs::write(parent.path().join("file"), b"payload")
        .expect("file fixture should be written");

    let reader_error = rooted
        .open_reader(Path::new("directory"), &LocalReadOptions::new())
        .expect_err("rooted directories must not open as readers");
    assert_eq!(LocalFileErrorKind::InvalidInput, reader_error.kind());
    let create_error = rooted
        .create_directory(
            Path::new("file"),
            &LocalCreateDirectoryOptions::new().with_exists_ok(),
        )
        .expect_err("an existing file cannot satisfy a directory request");
    assert!(matches!(
        create_error.kind(),
        LocalFileErrorKind::Io | LocalFileErrorKind::AlreadyExists
    ));

    let missing_file = rooted
        .delete_file(
            Path::new("missing-file"),
            &LocalDeleteOptions::new().with_missing_ok(),
        )
        .expect("missing rooted file should be accepted by policy");
    assert!(!missing_file.deleted());
    let missing_directory = rooted
        .delete_directory(
            Path::new("missing-directory"),
            &LocalDeleteOptions::new().with_missing_ok(),
        )
        .expect("missing rooted directory should be accepted by policy");
    assert!(!missing_directory.deleted());
}

/// Verifies rooted direct append reports its publication state when committed
/// and when explicitly aborted after accepting bytes.
#[test]
fn test_rooted_local_file_system_append_commit_and_abort_report_states() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");
    fs::write(parent.path().join("payload"), b"base")
        .expect("payload fixture should be written");

    let mut committed = rooted
        .open_writer(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect("rooted append writer should open");
    committed
        .write_all(b"-commit")
        .expect("rooted append writer should accept bytes");
    let committed_outcome = committed
        .commit()
        .expect("rooted append writer should commit");
    assert_eq!(LocalWriterState::Committed, committed_outcome.state());
    assert!(!committed_outcome.atomic());

    let mut aborted = rooted
        .open_writer(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect("second rooted append writer should open");
    aborted
        .write_all(b"-abort")
        .expect("second rooted append writer should accept bytes");
    let aborted_outcome = aborted.abort().expect("append abort should flush");
    assert_eq!(LocalWriterState::Published, aborted_outcome.state());
    assert_eq!(
        b"base-commit-abort",
        fs::read(parent.path().join("payload"))
            .expect("appended payload should read")
            .as_slice(),
    );
}

/// Verifies rooted temporary-resource validation rejects zero retry budgets.
#[test]
fn test_rooted_local_file_system_rejects_zero_temp_attempts() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");

    let file_error = rooted
        .create_temp_file(&LocalTempFileOptions::new().with_max_attempts(0))
        .expect_err("zero file retry budget must be rejected");
    assert_eq!(LocalFileErrorKind::InvalidInput, file_error.kind());
}

/// Verifies rooted listing refuses symlink following because
/// descriptor-relative traversal deliberately avoids path-resolution authority.
#[test]
fn test_rooted_local_file_system_rejects_follow_symlink_listing() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");

    let error = rooted
        .list(
            Path::new(""),
            &LocalListOptions::new().with_follow_symlinks(),
        )
        .expect_err("rooted traversal must reject symlink following");

    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.kind());
}

/// Verifies rooted walkers report a missing descendant and honor a zero depth
/// limit without opening any child entries.
#[test]
fn test_rooted_local_file_system_list_handles_missing_and_zero_depth() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");
    fs::write(parent.path().join("entry"), b"payload")
        .expect("entry fixture should be written");

    let missing_error = rooted
        .list(Path::new("missing"), &LocalListOptions::new())
        .expect_err("missing rooted directories must not open as walkers");
    assert_eq!(LocalFileErrorKind::NotFound, missing_error.kind());
    let entries = rooted
        .list(Path::new(""), &LocalListOptions::new().with_max_depth(0))
        .expect("rooted walker should open at the authority root")
        .collect::<Result<Vec<_>, _>>()
        .expect("zero-depth rooted traversal should succeed");
    assert!(entries.is_empty());
}

/// Verifies rooted recursive traversal stops descending once the configured
/// maximum entry depth is reached.
#[test]
fn test_rooted_local_file_system_recursive_listing_honors_max_depth() {
    let parent = tempdir().expect("root parent should be created");
    let nested = parent.path().join("first/second");
    fs::create_dir_all(&nested).expect("nested fixture should be created");
    fs::write(nested.join("payload"), b"payload")
        .expect("nested payload should be written");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");

    let entries = rooted
        .list(
            Path::new(""),
            &LocalListOptions::new().with_recursive().with_max_depth(1),
        )
        .expect("rooted walker should open")
        .collect::<Result<Vec<_>, _>>()
        .expect("rooted traversal should succeed");

    assert_eq!(1, entries.len());
    assert_eq!(Path::new("first"), entries[0].relative_path());
    assert_eq!(LocalFileKind::Directory, entries[0].metadata().kind());
}

/// Verifies recursive rooted traversal reports a child that changes from a
/// directory into a file after its parent listing has already observed it.
#[test]
fn test_rooted_local_file_system_reports_changed_child_during_recursive_list() {
    let parent = tempdir().expect("root parent should be created");
    let child = parent.path().join("child");
    fs::create_dir(&child).expect("child directory should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");
    let mut walker = rooted
        .list(Path::new(""), &LocalListOptions::new().with_recursive())
        .expect("rooted walker should open before the child changes");

    fs::remove_dir(&child).expect("empty child directory should be removed");
    fs::write(&child, b"replacement")
        .expect("child path should become a regular file");

    let error = walker
        .next()
        .expect("recorded child entry should be yielded")
        .expect_err("recursive descent into the changed child must fail");
    assert_eq!(LocalFileErrorKind::Io, error.kind());
    assert_eq!(Some(Path::new("child")), error.path());
}

/// Verifies rooted copy enforces directory recursion and atomicity policies
/// before publishing, then reports recursive success when both are accepted.
#[test]
fn test_rooted_local_file_system_copy_enforces_directory_policies() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");
    let source = parent.path().join("source");
    fs::create_dir(&source).expect("source directory should be created");
    fs::write(source.join("child"), b"payload")
        .expect("source child should be written");

    let file_only = rooted
        .copy(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_file_source(),
        )
        .expect_err("file-only copy must reject a directory source");
    assert_eq!(
        LocalFileErrorKind::RequirementNotMet,
        file_only.error().kind()
    );
    let atomic_required = rooted
        .copy(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new()
                .with_recursive()
                .with_atomicity(LocalAtomicityRequirement::Required),
        )
        .expect_err("directory copy cannot promise required atomicity");
    assert_eq!(
        LocalFileErrorKind::RequirementNotMet,
        atomic_required.error().kind()
    );

    let outcome = rooted
        .copy(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_recursive(),
        )
        .expect("recursive rooted copy should succeed");
    assert_eq!(LocalCopyMethod::Recursive, outcome.method());
    assert_eq!(
        b"payload",
        fs::read(parent.path().join("target/child"))
            .expect("copied child should read")
            .as_slice(),
    );
}

/// Verifies rooted directory creation, deletion, and temporary creation reject
/// lexical escape paths before native operations begin.
#[test]
fn test_rooted_local_file_system_rejects_lexical_escape_across_operations() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = RootedLocalFileSystem::open(parent.path())
        .expect("root authority should open");

    let create_error = rooted
        .create_directory(
            Path::new("../escape"),
            &LocalCreateDirectoryOptions::new(),
        )
        .expect_err("rooted directory creation must reject lexical escapes");
    assert_eq!(LocalFileErrorKind::InvalidInput, create_error.kind());
    let delete_error = rooted
        .delete_file(Path::new("../escape"), &LocalDeleteOptions::new())
        .expect_err("rooted file deletion must reject lexical escapes");
    assert_eq!(LocalFileErrorKind::InvalidInput, delete_error.kind());
    let temporary_error = rooted
        .create_temp_file(
            &LocalTempFileOptions::new().with_parent(Path::new("../escape")),
        )
        .expect_err("rooted temporary parents must reject lexical escapes");
    assert_eq!(LocalFileErrorKind::InvalidInput, temporary_error.kind());
}
