// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cross-platform contracts for compile-selected filesystem primitives.

use std::fs;
use std::path::Path;

use super::NamespaceHandle;
use crate::LocalFileErrorKind;
use crate::RelativePath;

/// Verifies path validation rejects a parent escape before native I/O starts.
#[test]
fn test_namespace_handle_rejects_parent_escape_before_io() {
    let sandbox = tempfile::tempdir().expect("sandbox should be created");
    let handle = NamespaceHandle::open_root(sandbox.path())
        .expect("sandbox root should open");

    let error = RelativePath::parse(Path::new("../escape"))
        .expect_err("parent escape should be rejected");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    drop(handle);
}

/// Verifies cloned authorities retain the opened root after its path is moved.
#[test]
fn test_namespace_handle_clone_survives_root_rename() {
    let sandbox = tempfile::tempdir().expect("sandbox should be created");
    let root = sandbox.path().join("root");
    let moved_root = sandbox.path().join("moved-root");
    fs::create_dir(&root).expect("root directory should be created");
    fs::write(root.join("entry"), b"original")
        .expect("rooted entry should be created");
    let handle =
        NamespaceHandle::open_root(&root).expect("root authority should open");
    let cloned = handle
        .clone_handle()
        .expect("root authority should be cloned");
    let entry = RelativePath::parse(Path::new("entry"))
        .expect("entry path should validate");

    fs::rename(&root, &moved_root).expect("root path should be renamed");

    assert_eq!(
        handle
            .metadata(&entry)
            .expect("original authority should retain entry")
            .len(),
        8
    );
    assert_eq!(
        cloned
            .metadata(&entry)
            .expect("cloned authority should retain entry")
            .len(),
        8
    );
}

/// Verifies identity observes replacement even when the lexical path is reused.
#[test]
fn test_entry_identity_distinguishes_same_path_replacement() {
    let sandbox = tempfile::tempdir().expect("sandbox should be created");
    let handle = NamespaceHandle::open_root(sandbox.path())
        .expect("sandbox root should open");
    let entry = RelativePath::parse(Path::new("entry"))
        .expect("entry path should validate");
    fs::write(sandbox.path().join("entry"), b"original")
        .expect("original entry should be created");
    let original = handle
        .entry_identity(&entry)
        .expect("original identity should be observed");

    fs::rename(
        sandbox.path().join("entry"),
        sandbox.path().join("retained-original"),
    )
    .expect("original entry should be retained under another name");
    fs::write(sandbox.path().join("entry"), b"replacement")
        .expect("replacement entry should be created");

    assert!(
        !original
            .matches_path(&handle, &entry)
            .expect("replacement identity should be compared")
    );
}
