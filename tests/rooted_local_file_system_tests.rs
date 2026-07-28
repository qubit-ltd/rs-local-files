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
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalFileErrorKind,
    LocalFileKind,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
    RootedLocalFileSystem,
};
use tempfile::tempdir;

/// Verifies rooted paths reject lexical escape components.
#[test]
fn test_rooted_local_file_system_rejects_lexical_escape() {
    let directory = tempdir().expect("temporary root should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let error = rooted
        .metadata(Path::new("../escape"))
        .expect_err("parent traversal must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidInput, error.kind());
}

/// Verifies rooted recursive listing remains within descriptor authority.
#[test]
fn test_rooted_local_file_system_lists_descendants() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("nested"))
        .expect("nested directory should be created");
    fs::write(directory.path().join("nested/child"), b"x")
        .expect("child should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let entries = rooted
        .list(
            Path::new("nested"),
            &LocalListOptions::new().with_recursive(),
        )
        .expect("rooted walker should be created")
        .collect::<Result<Vec<_>, _>>()
        .expect("rooted traversal should succeed");

    assert_eq!(1, entries.len());
    assert_eq!(Path::new("child"), entries[0].relative_path());
}

/// Verifies rooted writer publication and unified copy remain
/// descriptor-relative.
#[test]
fn test_rooted_local_file_system_writes_and_copies_file() {
    let directory = tempdir().expect("temporary root should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut writer = rooted
        .open_writer(
            Path::new("source"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted writer should open");
    writer
        .write_all(b"payload")
        .expect("staging write should succeed");
    let _outcome = writer.commit().expect("rooted commit should succeed");

    let outcome = rooted
        .copy(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new(),
        )
        .expect("rooted copy should succeed");
    assert_eq!(1, outcome.stats().files());
    assert_eq!(
        b"payload",
        fs::read(directory.path().join("target"))
            .expect("copied target should exist")
            .as_slice(),
    );
}

/// Verifies that rooted overwrite publication replaces a final symlink entry.
#[cfg(unix)]
#[test]
fn test_rooted_local_file_system_writer_replaces_final_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let target = directory.path().join("target");
    fs::write(&referent, b"original").expect("referent should be written");
    symlink("referent", &target).expect("target symlink should be created");
    let root = RootedLocalFileSystem::open(directory.path())
        .expect("rooted filesystem should open");

    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = root
        .open_writer(Path::new("target"), &options)
        .expect("rooted writer should accept the final symlink");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    let outcome = writer.commit().expect("replacement should publish");

    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert!(
        fs::symlink_metadata(&target)
            .expect("target metadata should exist")
            .is_file(),
    );
    assert_eq!(
        b"original".to_vec(),
        fs::read(&referent).expect("referent should remain unchanged"),
    );
}

/// Verifies rooted deletion distinguishes files and recursive directories.
#[test]
fn test_rooted_local_file_system_deletes_entries() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("tree"))
        .expect("tree should be created");
    fs::write(directory.path().join("tree/child"), b"x")
        .expect("child should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let outcome = rooted
        .delete_directory(
            Path::new("tree"),
            &LocalDeleteOptions::new().with_recursive(),
        )
        .expect("recursive rooted deletion should succeed");

    assert!(outcome.deleted());
    assert!(!directory.path().join("tree").exists());
}

/// Verifies the opened descriptor remains authoritative after diagnostic-path
/// rename.
#[test]
fn test_rooted_local_file_system_survives_root_path_rename() {
    let parent = tempdir().expect("temporary parent should be created");
    let original = parent.path().join("original");
    let renamed = parent.path().join("renamed");
    fs::create_dir(&original).expect("root fixture should be created");
    let rooted = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");
    fs::rename(&original, &renamed).expect("diagnostic path should be renamed");

    let _outcome = rooted
        .create_directory(
            Path::new("nested"),
            &LocalCreateDirectoryOptions::new(),
        )
        .expect("descriptor-relative creation should still succeed");

    assert!(renamed.join("nested").is_dir());
    assert!(!original.exists());
}

/// Verifies rooted metadata and reader operations share descriptor-relative
/// authority.
#[test]
fn test_rooted_local_file_system_reads_regular_file() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("payload"), b"content")
        .expect("fixture should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let metadata = rooted
        .metadata(Path::new("payload"))
        .expect("metadata should be read");
    assert_eq!(LocalFileKind::File, metadata.kind());

    let mut reader = rooted
        .open_reader(Path::new("payload"), &LocalReadOptions::new())
        .expect("reader should open");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("reader should read fixture");
    assert_eq!("content", content);
}

/// Verifies rooted rename defaults to no-replace.
#[test]
fn test_rooted_local_file_system_rename_respects_overwrite() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"new")
        .expect("source should be written");
    fs::write(directory.path().join("target"), b"old")
        .expect("target should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let error = rooted
        .rename(
            Path::new("source"),
            Path::new("target"),
            &LocalRenameOptions::new(),
        )
        .expect_err("default rename must not replace");
    assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());

    let _outcome = rooted
        .rename(
            Path::new("source"),
            Path::new("target"),
            &LocalRenameOptions::new().with_overwrite(),
        )
        .expect("explicit overwrite should succeed");
    assert_eq!(
        b"new",
        fs::read(directory.path().join("target"))
            .expect("target should be replaced")
            .as_slice(),
    );
}
