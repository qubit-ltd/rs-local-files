// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host I/O operation regression tests.

use std::fs;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalReadOptions;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;
use qubit_local_files::LocalWriterState;
use tempfile::tempdir;

/// Verifies the split Host I/O module preserves reader and writer behavior.
#[test]
fn test_host_io_operations_round_trip_file_bytes() {
    let directory = tempdir().expect("I/O test directory should exist");
    let path = directory.path().join("payload");
    let filesystem = LocalFileSystem::host();

    let mut writer = filesystem
        .open_writer(
            &path,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("Host writer should open");
    std::io::Write::write_all(&mut writer, b"payload")
        .expect("Host writer should accept bytes");
    let outcome = writer.commit().expect("Host writer should commit");
    assert_eq!(LocalWriterState::Committed, outcome.state());

    let bytes = filesystem
        .read_prefix(&path, &LocalReadOptions::new(), 32)
        .expect("Host reader should read committed bytes");
    assert_eq!(b"payload", bytes.as_slice());
    assert_eq!(b"payload", fs::read(path).expect("payload should remain readable").as_slice());
}
