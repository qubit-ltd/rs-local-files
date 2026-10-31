// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::error::LocalResult;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalFileKind;

fn populated_directory() -> (tempfile::TempDir, LocalDirectoryHandle) {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir(fixture.path().join("nested")).expect("nested directory should be created");
    std::fs::write(fixture.path().join("payload"), b"payload").expect("payload should be written");
    std::fs::write(fixture.path().join("nested/child"), b"child").expect("child should be written");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let directory = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("fixture directory should open");
    (fixture, directory)
}

#[test]
fn directory_handle_observes_entries_and_opens_bound_readers() {
    let (_fixture, directory) = populated_directory();

    assert_eq!(
        directory
            .observe()
            .expect("directory should be observed")
            .metadata()
            .kind(),
        LocalFileKind::Directory
    );
    assert_eq!(
        directory
            .entry_metadata(Path::new("payload"))
            .expect("metadata query should succeed")
            .expect("payload should exist")
            .len(),
        7
    );
    assert!(
        directory
            .entry_metadata(Path::new("missing"))
            .expect("missing metadata should be represented as absence")
            .is_none()
    );
    let security = directory.security_observation(&LocalSecurityObservationOptions::new());
    #[cfg(windows)]
    if let Err(error) = &security {
        assert_eq!(error.kind(), LocalFileErrorKind::PermissionDenied);
    }
    #[cfg(not(windows))]
    security.expect("optional security facts should be queried");
    assert_eq!(
        directory
            .observe_entry(Path::new("payload"))
            .expect("entry should be observed")
            .metadata()
            .len(),
        7
    );

    let mut reader = directory
        .open_observed_reader(
            Path::new("payload"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("observed reader should open");
    let bytes = reader.read_to_end_limited(16).expect("payload should be read");
    assert_eq!(bytes, b"payload");
    assert_eq!(
        reader
            .observe_now()
            .expect("reader should still observe its handle")
            .metadata()
            .len(),
        7
    );

    let nested = directory
        .open_directory(
            Path::new("nested"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("nested directory should open");
    assert!(
        nested
            .ancestor_observations(&LocalListOptions::new())
            .expect("ancestors should be observed")
            .len()
            >= 2
    );
    assert_eq!(
        nested
            .ancestor_observations(&LocalListOptions::new().with_max_depth(1))
            .expect_err("ancestor depth limit should be enforced")
            .kind(),
        LocalFileErrorKind::ResourceLimit,
    );
    assert!(reader.verify_path_binding().is_ok());
    let listed = directory
        .list_with_options(Path::new("."), &LocalListOptions::new().with_recursive())
        .expect("recursive listing should start")
        .collect::<LocalResult<Vec<_>>>()
        .expect("all entries should be listed");
    assert!(
        listed
            .iter()
            .any(|entry| entry.relative_path() == Path::new("nested/child"))
    );
}

#[test]
fn directory_handle_creates_writes_and_copies_with_options() {
    let (fixture, directory) = populated_directory();
    let created = directory
        .create_directory_with_options(Path::new("created"), &LocalCreateDirectoryOptions::new())
        .expect("directory should be created");
    assert!(created.created());

    let mut writer = directory
        .open_writer_with_options(
            Path::new("created/output"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("writer should open");
    qubit_io::Output::write_fully(&mut writer, b"written").expect("bytes should be written");
    let outcome = writer.commit().expect("writer should commit");
    assert_eq!(outcome.bytes_written(), 7);
    assert_eq!(
        std::fs::read(fixture.path().join("created/output")).expect("output should exist"),
        b"written"
    );

    let copied = directory
        .copy_with_options(Path::new("payload"), Path::new("copied"), &LocalCopyOptions::new())
        .expect("copy should succeed");
    assert_eq!(copied.stats().files(), 1);
    assert_eq!(
        std::fs::read(fixture.path().join("copied")).expect("copy should exist"),
        b"payload"
    );
}

#[test]
fn directory_handle_copy_zero_deadline_preserves_destination_and_parent() {
    let (fixture, directory) = populated_directory();
    std::fs::write(fixture.path().join("existing"), b"original")
        .expect("existing destination should be written");

    let failure = directory
        .copy_with_options(
            Path::new("payload"),
            Path::new("existing"),
            &LocalCopyOptions::new()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_deadline(std::time::Duration::ZERO),
        )
        .expect_err("zero copy budget should expire before copying");
    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.error().operation(), LocalFileOperation::Copy);
    assert_eq!(failure.error().io_error_kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(failure.partial_stats().files(), 0);
    assert_eq!(std::fs::read(fixture.path().join("existing")).unwrap(), b"original");

    let failure = directory
        .copy_with_options(
            Path::new("payload"),
            Path::new("new-parent/copied"),
            &LocalCopyOptions::new()
                .with_create_parent()
                .with_deadline(std::time::Duration::ZERO),
        )
        .expect_err("zero copy budget should not create a parent");
    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.error().operation(), LocalFileOperation::Copy);
    assert_eq!(failure.error().io_error_kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(failure.partial_stats().files(), 0);
    assert!(!fixture.path().join("new-parent").exists());
}
