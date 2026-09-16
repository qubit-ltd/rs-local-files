// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Prefix reads retain their outer operation throughout opening and reading.

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileOperation;
use tempfile::tempdir;

/// Checks lookup, syntax, and type failures without losing native causes or
/// coordinates.
#[test]
fn test_read_prefix_preserves_outer_operation_and_error_context() {
    let directory = tempdir().expect("fixture should exist");
    fs::write(directory.path().join("file"), b"payload").expect("file should exist");
    let host = LocalFileSystem::host().expect("Host should open");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted should open");
    for (filesystem, parent) in [(&host, directory.path()), (&rooted, Path::new("/"))] {
        for name in ["missing", "file/", "bad\0name", "."] {
            let path = parent.join(name);
            let opening = filesystem.open_reader(&path).expect_err("operand should be rejected");
            for limit in [0, 8] {
                let reading = filesystem
                    .read_prefix(&path, limit)
                    .expect_err("operand should be rejected");
                assert_eq!(opening.operation(), LocalFileOperation::OpenReader);
                assert_eq!(reading.operation(), LocalFileOperation::Read);
                assert_eq!(reading.kind(), opening.kind());
                assert_eq!(reading.cause_kind(), opening.cause_kind());
                assert_eq!(reading.io_error_kind(), opening.io_error_kind());
                assert_eq!(reading.path(), opening.path());
                assert_eq!(reading.current_directory(), opening.current_directory());
            }
        }
    }
}

/// Preserves typed native diagnostics for injected opening and read failures.
#[cfg(feature = "test-support")]
#[test]
fn test_read_prefix_fault_context() {
    use qubit_local_files::test_support::install_test_fault;

    let directory = tempdir().expect("fixture should exist");
    let path = directory.path().join("file");
    fs::write(&path, b"payload").expect("file should exist");
    let filesystem = LocalFileSystem::host().expect("Host should open");
    for point in [
        "local-fs-open-reader-metadata",
        "local-fs-open-reader-native",
        "local-fs-read-prefix-read",
    ] {
        let _fault = install_test_fault(point).expect("fault should install");
        let error = filesystem
            .read_prefix(&path, 8)
            .expect_err("fault should fail the prefix read");
        assert_eq!(error.operation(), LocalFileOperation::Read);
        assert_eq!(error.path(), Some(path.as_path()));
        assert!(error.typed_source().is_some());
    }
}
