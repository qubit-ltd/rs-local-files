// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::fs;
use std::io::Error;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;

use tempfile::tempdir;

use crate::local::absolute_path;
use crate::local::add_path_context;
use crate::local::canonicalize_existing_prefix;
use crate::local::clean_dir_path;
use crate::local::ensure_parent_path;
use crate::local::ensure_parent_path_with_sync_dirs;
use crate::local::remove_any_path;

/// Verifies lexical absolute paths are preserved and existing prefixes are
/// canonicalized without requiring the missing tail to exist.
#[test]
fn test_canonicalize_existing_prefix_preserves_missing_tail() {
    let directory = tempdir().expect("temporary directory should be created");
    let existing = directory.path().join("existing");
    fs::create_dir(&existing).expect("existing prefix should be created");
    let missing = existing.join("nested").join("payload");

    assert_eq!(
        fs::canonicalize(&existing)
            .expect("existing prefix should canonicalize")
            .join("nested")
            .join("payload"),
        canonicalize_existing_prefix(&missing).expect("missing tail should be preserved"),
    );
    assert_eq!(
        fs::canonicalize(&existing).expect("existing path should canonicalize"),
        canonicalize_existing_prefix(&existing).expect("existing path should canonicalize"),
    );
    assert_eq!(
        missing,
        absolute_path(&missing).expect("absolute input should be preserved")
    );
}

/// Verifies parent preparation reports every newly created directory in
/// shallow-to-deep order and performs no work for a leaf-only path.
#[test]
fn test_ensure_parent_path_reports_created_sync_directories() {
    let directory = tempdir().expect("temporary directory should be created");
    let first = directory.path().join("first");
    let second = first.join("second");
    let file = second.join("payload");

    assert_eq!(
        vec![first.clone(), second.clone()],
        ensure_parent_path_with_sync_dirs(&file).expect("missing parents should be created"),
    );
    assert!(second.is_dir());
    assert!(
        ensure_parent_path_with_sync_dirs(&file)
            .expect("existing parents should be accepted")
            .is_empty(),
    );
    ensure_parent_path(Path::new("payload")).expect("a leaf-only path has no parent to create");
}

/// Verifies added path context retains the native kind, source, operation,
/// and path for diagnostics.
#[test]
fn test_add_path_context_retains_native_error() {
    let native = Error::new(ErrorKind::PermissionDenied, "native denial");
    let error = add_path_context(native, "open", Path::new("private/payload"));

    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert!(error.to_string().contains("failed to open 'private/payload'"));
    assert!(error.source().is_some());
}

/// Verifies recursive cleanup keeps its root while removing files and
/// nested directories, and rejects a non-directory root.
#[test]
fn test_clean_dir_path_removes_children_but_keeps_root() {
    let directory = tempdir().expect("temporary directory should be created");
    let nested = directory.path().join("nested");
    fs::create_dir(&nested).expect("nested directory should be created");
    fs::write(nested.join("payload"), b"payload").expect("nested file should be created");
    fs::write(directory.path().join("sibling"), b"sibling").expect("sibling file should be created");

    clean_dir_path(directory.path()).expect("directory children should be removed");
    assert!(directory.path().is_dir());
    assert_eq!(
        0,
        fs::read_dir(directory.path())
            .expect("cleaned directory should remain readable")
            .count(),
    );

    let file = directory.path().join("file");
    fs::write(&file, b"payload").expect("file fixture should be created");
    assert_eq!(
        ErrorKind::InvalidInput,
        clean_dir_path(&file)
            .expect_err("a file cannot be cleaned as a directory")
            .kind(),
    );
}

/// Verifies generic removal deletes files and directory trees.
#[test]
fn test_remove_any_path_handles_files_and_directories() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("file");
    fs::write(&file, b"payload").expect("file fixture should be created");
    remove_any_path(&file).expect("file should be removed");
    assert!(!file.exists());

    let tree = directory.path().join("tree");
    fs::create_dir(&tree).expect("directory fixture should be created");
    fs::write(tree.join("payload"), b"payload").expect("tree file should be created");
    remove_any_path(&tree).expect("directory tree should be removed");
    assert!(!tree.exists());
}

/// Verifies removing a symbolic link never removes its referent.
#[cfg(unix)]
#[test]
fn test_remove_any_path_unlinks_symbolic_link_only() {
    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let link = directory.path().join("link");
    fs::write(&referent, b"payload").expect("referent should be created");
    symlink(&referent, &link).expect("symbolic link should be created");

    remove_any_path(&link).expect("symbolic link should be removed");
    assert!(!link.exists());
    assert!(referent.exists());
}
