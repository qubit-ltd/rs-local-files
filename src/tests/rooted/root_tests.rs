// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for rooted directory authority operations.

use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use tempfile::tempdir;

use crate::LocalRelativePath;
use crate::rooted::EntryKind;
use crate::rooted::Root;

/// Verifies an opened root performs descriptor-relative namespace operations
/// without consulting the diagnostic path after opening.
#[test]
fn test_root_authority_manages_descendant_entries() {
    let directory = tempdir().expect("temporary root should be created");
    let root = Root::open(directory.path()).expect("root should open");
    let nested = LocalRelativePath::new(Path::new("nested"))
        .expect("nested path should be valid");
    let file = LocalRelativePath::new(Path::new("nested/payload"))
        .expect("file path should be valid");
    let renamed = LocalRelativePath::new(Path::new("nested/renamed"))
        .expect("renamed path should be valid");

    root.create_dir(&nested).expect("nested directory should be created");
    root.ensure_dir(&nested).expect("existing directory should be accepted");
    fs::write(directory.path().join(file.as_path()), b"payload")
        .expect("fixture file should be written");

    assert_eq!(EntryKind::Directory, root.metadata().expect("root metadata").kind());
    assert_eq!(EntryKind::File, root.symlink_metadata(&file).expect("file metadata").kind());
    root.open_probe_file(&file).expect("file should be probeable");
    root.open_probe_file(&nested).expect("directory should be probeable");
    root.open_probe_root().expect("root should be probeable");
    assert_eq!(1, root.read_dir(&nested).expect("nested entries").len());
    assert_eq!(1, root.read_root_dir().expect("root entries").len());

    let mut reader = root.open_dir_reader(&nested).expect("nested reader");
    assert!(reader.next_entry().expect("reader entry").is_some());
    assert!(reader.next_entry().expect("reader exhaustion").is_none());

    root.rename_without_replacing(&file, &renamed)
        .expect("file should rename without replacement");
    root.rename(&renamed, &file).expect("file should rename with replacement");
    let mut reader = root
        .open_reader(&file, &crate::read::OpenOptions::default())
        .expect("rooted reader should open");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("rooted reader should read fixture content");
    assert_eq!("payload", content);
    let appended = LocalRelativePath::new(Path::new("nested/appended"))
        .expect("appended path should be valid");
    root.open_writer(&appended, &crate::write::OpenOptions::default())
        .expect("rooted writer should open")
        .write_all(b"appended")
        .expect("rooted writer should write fixture bytes");
    root.remove_file(&appended).expect("appended file should be removed");
    root.remove_file(&file).expect("file should be removed");
    root.remove_empty_dir(&nested).expect("empty directory should be removed");

    let tree = LocalRelativePath::new(Path::new("tree/child"))
        .expect("tree path should be valid");
    root.ensure_dir_all(&tree).expect("tree should be created");
    root.remove_tree(
        &LocalRelativePath::new(Path::new("tree"))
            .expect("tree root should be valid"),
    )
    .expect("tree should be removed");
}
