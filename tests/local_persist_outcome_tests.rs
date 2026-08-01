// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public persistence outcome coverage.

use std::io::Write;

use qubit_local_files::{
    LocalFileSystem, LocalPersistMethod, LocalPersistOptions, LocalTempFileOptions,
};

/// Verifies outcome accessors report the completed temporary-file publication.
#[test]
fn test_local_persist_outcome_reports_published_path_and_guarantees() {
    let root = tempfile::tempdir().expect("test root must be created");
    let target = root.path().join("target.txt");
    let mut temporary =
        LocalFileSystem::create_temp_file(&LocalTempFileOptions::new().with_parent(root.path()))
            .expect("temporary file must be created");
    temporary
        .write_all(b"payload")
        .expect("temporary file must be writable");

    let outcome = temporary
        .persist_with_outcome(&target, LocalPersistOptions::new())
        .expect("temporary file must persist");

    assert_eq!(target, outcome.path());
    assert_eq!(LocalPersistMethod::AtomicRename, outcome.method());
    assert!(outcome.atomic());
    assert!(!outcome.durable());
    assert_eq!(target, outcome.into_path());
}
