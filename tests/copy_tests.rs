// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::copy;

/// Verifies copy options are exposed through the responsibility module.
#[test]
fn test_copy_options_are_available() {
    let _ = copy::Options::default();
}

/// Verifies recursive copying reports the copied tree.
#[test]
fn test_copy_directory_copies_regular_files() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::create_dir(&source).expect("the source should be created");
    std::fs::write(source.join("payload"), b"payload")
        .expect("the source fixture should be written");

    let statistics =
        copy::directory(&source, &destination, copy::Options::default())
            .expect("the source tree should be copied");

    assert_eq!(1, statistics.files());
    assert_eq!(7, statistics.bytes());
    assert_eq!(
        b"payload",
        std::fs::read(destination.join("payload"))
            .unwrap()
            .as_slice()
    );
}

/// Verifies file copy reports bytes and replaces an existing destination.
#[test]
fn test_copy_file_replaces_existing_destination() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::write(&source, b"source").expect("the source should be written");
    std::fs::write(&destination, b"destination")
        .expect("the destination should be written");

    let bytes = copy::file(&source, &destination)
        .expect("the destination should be replaced");

    assert_eq!(6, bytes);
    assert_eq!(b"source", std::fs::read(&destination).unwrap().as_slice());
}

/// Verifies no-replace file copy leaves an existing destination unchanged.
#[test]
fn test_copy_file_without_replacing_preserves_destination() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::write(&source, b"source").expect("the source should be written");
    std::fs::write(&destination, b"destination")
        .expect("the destination should be written");

    let error = copy::file_without_replacing(&source, &destination)
        .expect_err("the destination conflict should be rejected");

    assert_eq!(std::io::ErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        b"destination",
        std::fs::read(&destination).unwrap().as_slice(),
    );
}

/// Verifies no-replace file copy creates a missing destination and preserves
/// portable permissions.
#[cfg(unix)]
#[test]
fn test_copy_file_without_replacing_creates_destination() {
    use std::os::unix::fs::{
        MetadataExt,
        PermissionsExt,
    };

    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::write(&source, b"source").expect("the source should be written");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o640))
        .expect("the source permissions should be set");

    let bytes = copy::file_without_replacing(&source, &destination)
        .expect("the destination should be created");

    assert_eq!(6, bytes);
    assert_eq!(b"source", std::fs::read(&destination).unwrap().as_slice());
    assert_eq!(
        0o640,
        std::fs::metadata(&destination)
            .expect("the destination metadata should be readable")
            .mode()
            & 0o777,
    );
}

/// Verifies a failed no-replace file copy removes its newly created partial
/// destination.
#[cfg(unix)]
#[test]
fn test_copy_file_without_replacing_cleans_up_failed_destination() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source-directory");
    let destination = directory.path().join("destination");
    std::fs::create_dir(&source).expect("the source directory should exist");

    copy::file_without_replacing(&source, &destination)
        .expect_err("copying directory bytes should fail");

    assert!(!destination.exists());
}

/// Verifies a missing source is reported before a destination is created.
#[test]
fn test_copy_file_without_replacing_rejects_missing_source() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("missing");
    let destination = directory.path().join("destination");

    let error = copy::file_without_replacing(&source, &destination)
        .expect_err("the missing source should be rejected");

    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    assert!(!destination.exists());
}

/// Verifies a permission-copy failure retains the original error and removes
/// the newly created destination entry.
#[cfg(unix)]
#[test]
fn test_copy_file_without_replacing_cleans_up_permission_failure() {
    use std::{
        ffi::CString,
        io::Write,
        os::unix::{
            ffi::OsStrExt,
            fs::symlink,
        },
        thread,
        time::Duration,
    };

    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source-fifo");
    let destination = directory.path().join("destination");
    let source_c = CString::new(source.as_os_str().as_bytes())
        .expect("the FIFO path should not contain NUL");
    // SAFETY: `source_c` is a live NUL-terminated path for this non-retaining
    // call.
    let result = unsafe { libc::mkfifo(source_c.as_ptr(), 0o600) };
    assert_eq!(0, result, "the source FIFO should be created");

    let writer_source = source.clone();
    let writer = thread::spawn(move || {
        let mut pipe = std::fs::OpenOptions::new()
            .write(true)
            .open(writer_source)
            .expect("the FIFO writer should open");
        thread::sleep(Duration::from_millis(50));
        pipe.write_all(b"payload")
            .expect("the FIFO payload should be written");
    });
    let replacement_destination = destination.clone();
    let replacer = thread::spawn(move || {
        while !replacement_destination.exists() {
            thread::yield_now();
        }
        std::fs::remove_file(&replacement_destination)
            .expect("the opened destination entry should be removed");
        symlink("missing-target", &replacement_destination)
            .expect("a dangling replacement link should be created");
    });

    let error = copy::file_without_replacing(&source, &destination)
        .expect_err("permission copying through the replacement should fail");

    writer.join().expect("the FIFO writer should finish");
    replacer
        .join()
        .expect("the destination replacer should finish");
    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    assert!(!destination.exists());
}
