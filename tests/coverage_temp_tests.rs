// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSlice;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;

use qubit_io::Output;
use qubit_local_files::LocalDirectoryWalker;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalResult;
use tempfile::tempdir;

#[test]
fn test_temp_file_accessors_writes_and_keep_cover_lifecycle() {
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem.create_temp_file().expect("temporary file");
    assert!(temporary.creation_parent().is_absolute());
    assert!(temporary.path().starts_with(temporary.creation_parent()));

    let written = temporary
        .write_vectored(&[IoSlice::new(b"first"), IoSlice::new(b"-second")])
        .expect("vectored write");
    assert!(written > 0);
    Output::flush(&mut temporary).expect("flush temporary file");
    temporary.seek(SeekFrom::Start(0)).expect("seek temporary file");

    let outcome = temporary.keep().expect("keep temporary file");
    assert!(outcome.path().exists());
    assert_eq!(std::fs::read(outcome.path()).expect("read kept file").len(), written);
}

#[test]
fn test_filesystem_rename_and_directory_listing_entry_points() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let directory = fixture.path().join("directory");
    let _created = filesystem.create_directory(&directory).expect("create directory");
    let source = directory.join("source");
    let destination = directory.join("destination");
    std::fs::write(&source, b"payload").expect("source fixture");

    let _outcome = filesystem.rename(&source, &destination).expect("rename");
    let entries = filesystem.list(&directory).expect("list directory");
    let root = std::hint::black_box(LocalDirectoryWalker::root as for<'a> fn(&'a LocalDirectoryWalker) -> &'a Path);
    assert_eq!(directory, root(&entries), "walker exposes its bound traversal root");
    let entries = entries.collect::<LocalResult<Vec<_>>>().expect("collect entries");

    assert_eq!(1, entries.len());
    assert_eq!(
        b"payload",
        std::fs::read(destination).expect("renamed bytes").as_slice()
    );
}
