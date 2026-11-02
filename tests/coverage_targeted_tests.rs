// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSlice;
use std::io::IoSliceMut;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use tempfile::tempdir;

#[test]
fn test_local_file_writer_vectored_write_publishes_ordered_buffers() {
    let fixture = tempdir().expect("fixture");
    let target = fixture.path().join("vectored");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut writer = filesystem
        .open_writer_with_options(&target, &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("writer");
    let written = writer
        .write_vectored(&[IoSlice::new(b"first"), IoSlice::new(b"-"), IoSlice::new(b"second")])
        .expect("vectored write");
    assert!((1..=12).contains(&written));
    let _outcome = writer.commit().expect("publish write");
    assert_eq!(std::fs::read(target).expect("published bytes"), b"first-second");
}

#[test]
fn test_local_file_reader_vectored_read_fills_ordered_buffers() {
    let fixture = tempdir().expect("fixture");
    let target = fixture.path().join("vectored");
    std::fs::write(&target, b"first-second").expect("fixture bytes");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut reader = filesystem.open_reader(Path::new(&target)).expect("reader");
    let mut first = [0; 5];
    let mut separator = [0; 1];
    let mut second = [0; 6];
    let count = reader
        .read_vectored(&mut [
            IoSliceMut::new(&mut first),
            IoSliceMut::new(&mut separator),
            IoSliceMut::new(&mut second),
        ])
        .expect("vectored read");
    assert_eq!(count, 12);
    assert_eq!(&first, b"first");
    assert_eq!(&separator, b"-");
    assert_eq!(&second, b"second");
}

#[test]
fn test_rooted_path_lexical_escape_is_rejected_with_namespace_context() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let error = filesystem
        .canonicalize(Path::new("../outside"))
        .expect_err("lexical parent must not leave the virtual root");
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::InvalidPath);
    assert_eq!(error.path(), Some(Path::new("../outside")));
}

#[test]
fn test_directory_qualified_file_operand_reports_not_directory() {
    let fixture = tempdir().expect("fixture");
    let file = fixture.path().join("file");
    std::fs::write(&file, b"payload").expect("file fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let directory_qualified = Path::new(&format!("{}/", file.display())).to_path_buf();
    let error = filesystem
        .target_metadata(&directory_qualified)
        .expect_err("a file cannot satisfy a directory-qualified operand");
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::NotDirectory);
}

#[test]
fn test_staged_directory_debug_reports_state_without_exposing_native_path() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");

    let debug = format!("{stage:?}");
    assert!(debug.contains("LocalStagedDirectory"));
    assert!(debug.contains("owner_present: true"));
    assert!(!debug.contains(&fixture.path().to_string_lossy().to_string()));
    stage.cleanup().expect("cleanup stage");
}
