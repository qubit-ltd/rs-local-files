// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Windows Rooted atomic-writer identity-race integration tests.

#![cfg(all(windows, feature = "test-support"))]

use std::fs;
use std::path::Path;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalWriteFailureState;
use qubit_local_files::outcome::LocalWriterState;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

/// A destination replaced after its commit-time handle is opened remains
/// untouched, while the pre-publication failure retains its staging writer.
#[test]
fn rooted_writer_destination_replacement_before_install_is_not_published() {
    let directory = tempdir().expect("temporary directory should be created");
    let destination = directory.path().join("payload");
    fs::write(&destination, b"original destination")
        .expect("original destination should be written");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");
    let mut writer = rooted
        .open_writer_with_options(
            Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("replacement writer should open");
    Output::write_fully(&mut writer, b"staged replacement")
        .expect("writer should accept staged data");

    let _fault = install_test_fault("rooted-writer-identity-replace")
        .expect("identity replacement hook should be installed");
    let failure = writer
        .commit()
        .expect_err("identity mismatch must prevent publication");

    assert_eq!(LocalWriteFailureState::NotPublished, failure.state());
    let (error, state, retained) = failure.into_parts();
    assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        Some(std::io::ErrorKind::AlreadyExists),
        error.io_error().map(std::io::Error::kind)
    );
    assert_eq!(LocalWriteFailureState::NotPublished, state);
    assert_eq!(
        b"competing destination",
        fs::read(&destination)
            .expect("competing destination should remain")
            .as_slice()
    );
    let mut retained = retained.expect("pre-publication writer should be retained");
    assert_eq!(LocalWriterState::Open, retained.state());
    assert_eq!(
        2,
        fs::read_dir(directory.path())
            .expect("root should be readable")
            .count()
    );
    let aborted = retained
        .abort()
        .expect("retained staging should abort cleanly");
    assert_eq!(LocalWriterState::Aborted, aborted.state());
    assert_eq!(
        b"competing destination",
        fs::read(&destination)
            .expect("abort should preserve competing destination")
            .as_slice()
    );
    assert_eq!(
        1,
        fs::read_dir(directory.path())
            .expect("root should be readable")
            .count()
    );
}
