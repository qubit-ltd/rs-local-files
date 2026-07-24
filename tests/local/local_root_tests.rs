// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::io::{
    ErrorKind,
    Read,
    Write,
};

#[cfg(unix)]
use qubit_local_files::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalRelativePath,
    LocalRoot,
};

#[cfg(target_os = "linux")]
use super::test_support::SourceReadLease;
#[cfg(all(coverage, target_os = "linux"))]
use super::test_support::run_in_coverage_fault_process;
#[cfg(unix)]
use super::test_support::{
    create_fifo,
    fs,
    temp_dir,
};

/// Verifies one injected root-handle metadata or type failure.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_root_open_error(
    test_name: &str,
    fault: &str,
    expected_kind: Option<ErrorKind>,
) {
    let Some(()) = run_in_coverage_fault_process(test_name, fault, move || {
        let root_path = temp_dir(fault);
        let error = LocalRoot::open(&root_path)
            .expect_err("injected root validation should fail");
        if let Some(expected_kind) = expected_kind {
            assert_eq!(expected_kind, error.kind());
        } else {
            assert!(
                error.to_string().contains("Input/output error"),
                "contextual error should retain the injected native failure: {error}",
            );
        }
        fs::remove_dir_all(root_path)
            .expect("test directory should be removed");
    }) else {
        return;
    };
}

/// Verifies propagation of injected root-directory metadata failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_root_reports_injected_directory_metadata_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_tests::",
        "test_open_root_reports_injected_directory_metadata_error",
    );
    assert_injected_root_open_error(
        TEST_NAME,
        "rooted-directory-metadata",
        None,
    );
}

/// Verifies rejection of an injected invalid root-directory type.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_root_reports_injected_directory_type_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_tests::",
        "test_open_root_reports_injected_directory_type_error",
    );
    assert_injected_root_open_error(
        TEST_NAME,
        "rooted-directory-type",
        Some(ErrorKind::InvalidInput),
    );
}

/// Asserts one injected rooted file-handle validation failure.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_rooted_reader_error(
    test_name: &str,
    fault: &str,
    expected_kind: Option<ErrorKind>,
) {
    let Some(()) = run_in_coverage_fault_process(test_name, fault, move || {
        let root_path = temp_dir(fault);
        fs::write(root_path.join("data.txt"), b"data")
            .expect("file fixture should be written");
        let root = LocalRoot::open(&root_path).expect("root should open");
        let path = LocalRelativePath::new("data.txt")
            .expect("relative path should validate");

        let error = root
            .open_reader(&path, FileReadOptions::unbuffered())
            .expect_err("injected rooted reader validation should fail");

        if let Some(expected_kind) = expected_kind {
            assert_eq!(expected_kind, error.kind());
        } else {
            assert!(
                error.to_string().contains("Input/output error"),
                "contextual error should retain the injected native failure: {error}",
            );
        }
        fs::remove_dir_all(root_path)
            .expect("test directory should be removed");
    }) else {
        return;
    };
}

/// Verifies propagation of injected rooted file metadata failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_reader_reports_injected_file_metadata_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_tests::",
        "test_open_reader_reports_injected_file_metadata_error",
    );
    assert_injected_rooted_reader_error(
        TEST_NAME,
        "rooted-file-metadata",
        None,
    );
}

/// Verifies rejection of an injected invalid rooted file type.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_reader_reports_injected_file_type_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_tests::",
        "test_open_reader_reports_injected_file_type_error",
    );
    assert_injected_rooted_reader_error(
        TEST_NAME,
        "rooted-file-type",
        Some(ErrorKind::InvalidInput),
    );
}

/// Verifies contextual propagation of injected final-entry inspection errors.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_reader_reports_injected_entry_inspection_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_tests::",
        "test_open_reader_reports_injected_entry_inspection_error",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-entry-inspect",
        || {
            let root_path = temp_dir("rooted-entry-inspect");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let path = LocalRelativePath::new("missing.txt")
                .expect("relative path should validate");

            let error = root
                .open_reader(&path, FileReadOptions::unbuffered())
                .expect_err("injected entry inspection should fail");

            assert!(
                error.to_string().contains("Input/output error"),
                "contextual error should retain the injected native failure: {error}",
            );
            fs::remove_dir_all(root_path)
                .expect("test directory should be removed");
        },
    ) else {
        return;
    };
}

/// Verifies that rooted handle validation fails before destructive truncation.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_writer_preserves_contents_when_rooted_validation_fails() {
    const TEST_NAME: &str = concat!(
        "local::local_root_tests::",
        "test_open_writer_preserves_contents_when_rooted_validation_fails",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-file-metadata",
        || {
            let root_path = temp_dir("rooted-writer-metadata");
            let destination_path = root_path.join("data.txt");
            fs::write(&destination_path, b"original")
                .expect("rooted writer fixture should be written");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let path = LocalRelativePath::new("data.txt")
                .expect("relative path should validate");
            let error = root
                .open_writer(&path, FileWriteOptions::default())
                .expect_err("injected rooted validation should fail");

            assert!(
                error.to_string().contains("inspect rooted file handle"),
                "validation error should retain operation context: {error}",
            );
            assert_eq!(
                b"original",
                fs::read(&destination_path)
                    .expect("failed rooted destination should remain readable")
                    .as_slice(),
            );
            fs::remove_dir_all(root_path)
                .expect("rooted writer fixture should be removed");
        },
    ) else {
        return;
    };
}

/// Verifies every rooted write mode and buffered flushing against an anchored
/// parent descriptor.
#[cfg(unix)]
#[test]
fn test_open_writer_supports_all_modes() {
    let root_path = temp_dir("rooted-write-modes");
    fs::write(root_path.join("existing.txt"), b"abc")
        .expect("existing fixture should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let existing = LocalRelativePath::new("existing.txt")
        .expect("existing path should validate");

    let mut writer = root
        .open_writer(
            &existing,
            FileWriteOptions::new(FileWriteMode::OpenExistingAtStart),
        )
        .expect("existing file should open at its start");
    writer.write_all(b"X").expect("prefix should be replaced");
    writer.close().expect("writer should close");

    let error = root
        .open_writer(&existing, FileWriteOptions::new(FileWriteMode::CreateNew))
        .expect_err("create-new should reject an existing destination");
    assert_eq!(ErrorKind::AlreadyExists, error.kind());

    let mut writer = root
        .open_writer(
            &existing,
            FileWriteOptions::new(FileWriteMode::AppendExisting),
        )
        .expect("existing file should open for append");
    writer.write_all(b"Y").expect("suffix should append");
    writer.close().expect("append writer should close");

    let created = LocalRelativePath::new("created.txt")
        .expect("created path should validate");
    let error = root
        .open_writer(
            &created,
            FileWriteOptions::new(FileWriteMode::AppendExisting),
        )
        .expect_err("append-existing should reject a missing destination");
    assert_eq!(ErrorKind::NotFound, error.kind());

    let mut writer = root
        .open_writer(
            &created,
            FileWriteOptions::new(FileWriteMode::AppendOrCreate).buffered(),
        )
        .expect("append-or-create should create a buffered writer");
    writer
        .write_all(b"Z")
        .expect("created file should be written");
    writer.flush().expect("buffered writer should flush");
    writer.close().expect("buffered writer should close");

    let new_path =
        LocalRelativePath::new("new.txt").expect("new path should validate");
    root.open_writer(
        &new_path,
        FileWriteOptions::new(FileWriteMode::CreateNew),
    )
    .expect("create-new should create a missing file")
    .close()
    .expect("new writer should close");

    assert_eq!(
        b"XbcY",
        fs::read(root_path.join("existing.txt")).unwrap().as_slice(),
    );
    assert_eq!(
        b"Z",
        fs::read(root_path.join("created.txt")).unwrap().as_slice(),
    );
    fs::remove_dir_all(root_path)
        .expect("write-mode fixture should be removed");
}

/// Verifies deterministic errors for missing components and non-file resource
/// types, including FIFOs that must never block the caller.
#[cfg(unix)]
#[test]
fn test_rooted_io_rejects_missing_and_wrong_resource_types() {
    let root_path = temp_dir("rooted-resource-types");
    fs::create_dir(root_path.join("directory"))
        .expect("directory fixture should be created");
    fs::write(root_path.join("parent-file"), b"file")
        .expect("parent file fixture should be written");
    create_fifo(&root_path.join("pipe"));
    let root = LocalRoot::open(&root_path).expect("root should open");

    let missing = LocalRelativePath::new("missing/file.txt")
        .expect("missing path should validate");
    let error = root
        .open_reader(&missing, FileReadOptions::unbuffered())
        .expect_err("missing reader parent should fail");
    assert_eq!(ErrorKind::NotFound, error.kind());
    let error = root
        .open_writer(&missing, FileWriteOptions::default())
        .expect_err("missing writer parent should fail without creation");
    assert_eq!(ErrorKind::NotFound, error.kind());

    for invalid in ["directory", "pipe"] {
        let path = LocalRelativePath::new(invalid)
            .expect("resource path should validate");
        assert_eq!(
            ErrorKind::InvalidInput,
            root.open_reader(&path, FileReadOptions::unbuffered())
                .expect_err("reader should reject a non-file")
                .kind(),
        );
        assert_eq!(
            ErrorKind::InvalidInput,
            root.open_writer(&path, FileWriteOptions::default())
                .expect_err("writer should reject a non-file")
                .kind(),
        );
    }

    let invalid_parent = LocalRelativePath::new("parent-file/child.txt")
        .expect("invalid parent path should validate lexically");
    let error = root
        .open_writer(&invalid_parent, FileWriteOptions::default().with_parent())
        .expect_err("ordinary file parent should be rejected");
    assert_eq!(ErrorKind::InvalidInput, error.kind());

    fs::remove_dir_all(root_path)
        .expect("resource-type fixture should be removed");
}

/// Verifies root opening distinguishes missing paths from existing non-
/// directories.
#[cfg(unix)]
#[test]
fn test_open_root_rejects_missing_path_and_regular_file() {
    let fixture = temp_dir("rooted-open-errors");
    let regular_file = fixture.join("file.txt");
    fs::write(&regular_file, b"file").expect("file fixture should be written");

    assert_eq!(
        ErrorKind::NotFound,
        LocalRoot::open(fixture.join("missing"))
            .expect_err("missing root should fail")
            .kind(),
    );
    assert_eq!(
        ErrorKind::InvalidInput,
        LocalRoot::open(&regular_file)
            .expect_err("regular-file root should fail")
            .kind(),
    );

    fs::remove_dir_all(fixture).expect("root-error fixture should be removed");
}

/// Verifies anchored reader and writer success, including secure parent
/// creation for a nested writer target.
#[cfg(unix)]
#[test]
fn test_open_reader_and_writer_use_root_capability() {
    let root_path = temp_dir("rooted-file-io");
    fs::write(root_path.join("input.txt"), b"input")
        .expect("rooted reader fixture should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");

    let input = LocalRelativePath::new("input.txt")
        .expect("reader path should validate");
    let mut reader = root
        .open_reader(&input, FileReadOptions::buffered())
        .expect("rooted reader should open");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("rooted reader should read");

    let output = LocalRelativePath::new("nested/output.txt")
        .expect("writer path should validate");
    let mut writer = root
        .open_writer(&output, FileWriteOptions::default().with_parent())
        .expect("rooted writer should create parents");
    writer
        .write_all(b"output")
        .expect("rooted writer should write");
    writer.close().expect("rooted writer should close");

    assert_eq!("input", content);
    assert_eq!(root_path.as_path(), root.path());
    assert_eq!(
        b"output",
        fs::read(root_path.join("nested/output.txt"))
            .expect("rooted output should be readable")
            .as_slice(),
    );
    fs::remove_dir_all(root_path).expect("rooted fixture should be removed");
}

/// Verifies rooted readers preserve blocking semantics for Linux file leases.
#[cfg(target_os = "linux")]
#[test]
fn test_open_rooted_reader_waits_for_conflicting_file_lease() {
    let root_path = temp_dir("rooted-reader-file-lease");
    let path = root_path.join("data.txt");
    fs::write(&path, b"payload").expect("reader fixture should be written");
    let lease = SourceReadLease::acquire(&path)
        .expect("write lease should be acquired");
    let worker_root_path = root_path.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let root = LocalRoot::open(&worker_root_path)
            .expect("worker root should open");
        let relative = LocalRelativePath::new("data.txt")
            .expect("reader path should validate");
        sender
            .send(root.open_reader(&relative, FileReadOptions::unbuffered()))
            .expect("reader result should be sent");
    });

    lease
        .wait_for_break()
        .expect("rooted reader should request a lease break");
    let early_result =
        receiver.recv_timeout(std::time::Duration::from_millis(250));
    lease.release().expect("write lease should be released");
    let result = match early_result {
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("reader result should arrive after lease release"),
        Err(error) => {
            panic!("reader worker disconnected unexpectedly: {error}")
        }
        Ok(result) => {
            worker.join().expect("reader worker should not panic");
            panic!("rooted reader returned before lease release: {result:?}");
        }
    };
    worker.join().expect("reader worker should not panic");
    let reader = result.expect("rooted reader should open after lease release");

    drop(reader);
    fs::remove_dir_all(root_path).expect("reader fixture should be removed");
}

/// Verifies rooted writers preserve blocking semantics for Linux file leases.
#[cfg(target_os = "linux")]
#[test]
fn test_open_rooted_writer_waits_for_conflicting_file_lease() {
    let root_path = temp_dir("rooted-writer-file-lease");
    let path = root_path.join("data.txt");
    fs::write(&path, b"payload").expect("writer fixture should be written");
    let lease = SourceReadLease::acquire(&path)
        .expect("write lease should be acquired");
    let worker_root_path = root_path.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let root = LocalRoot::open(&worker_root_path)
            .expect("worker root should open");
        let relative = LocalRelativePath::new("data.txt")
            .expect("writer path should validate");
        sender
            .send(root.open_writer(
                &relative,
                FileWriteOptions::new(FileWriteMode::OpenExistingAtStart),
            ))
            .expect("writer result should be sent");
    });

    lease
        .wait_for_break()
        .expect("rooted writer should request a lease break");
    let early_result =
        receiver.recv_timeout(std::time::Duration::from_millis(250));
    lease.release().expect("write lease should be released");
    let result = match early_result {
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("writer result should arrive after lease release"),
        Err(error) => {
            panic!("writer worker disconnected unexpectedly: {error}")
        }
        Ok(result) => {
            worker.join().expect("writer worker should not panic");
            panic!("rooted writer returned before lease release: {result:?}");
        }
    };
    worker.join().expect("writer worker should not panic");
    let writer = result.expect("rooted writer should open after lease release");

    drop(writer);
    fs::remove_dir_all(root_path).expect("writer fixture should be removed");
}

/// Verifies rooted file opens can bound Linux lease-conflict retries.
#[cfg(target_os = "linux")]
#[test]
fn test_open_rooted_reader_and_writer_timeout_report_lease_conflicts() {
    let root_path = temp_dir("rooted-open-lease-timeout");
    let path = root_path.join("data.txt");
    fs::write(&path, b"payload").expect("fixture should be written");

    for is_writer in [false, true] {
        let lease = SourceReadLease::acquire(&path)
            .expect("write lease should be acquired");
        let worker_root_path = root_path.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let root = LocalRoot::open(&worker_root_path)
                .expect("worker root should open");
            let relative = LocalRelativePath::new("data.txt")
                .expect("path should validate");
            let result = if is_writer {
                root.open_writer(
                    &relative,
                    FileWriteOptions::new(FileWriteMode::OpenExistingAtStart)
                        .with_open_retry_timeout(std::time::Duration::ZERO),
                )
                .map(drop)
            } else {
                root.open_reader(
                    &relative,
                    FileReadOptions::unbuffered()
                        .with_open_retry_timeout(std::time::Duration::ZERO),
                )
                .map(drop)
            };
            sender.send(result).expect("open result should be sent");
        });

        lease
            .wait_for_break()
            .expect("rooted open should request a lease break");
        let error = receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("timed rooted result should arrive")
            .expect_err("zero timeout should reject the lease conflict");
        assert_eq!(ErrorKind::TimedOut, error.kind());
        lease.release().expect("write lease should be released");
        worker.join().expect("rooted worker should not panic");
    }

    fs::remove_dir_all(root_path).expect("fixture should be removed");
}

/// Verifies that renaming the diagnostic path cannot redirect an already open
/// root capability.
#[cfg(unix)]
#[test]
fn test_open_reader_survives_root_rename() {
    let root_path = temp_dir("rooted-rename");
    fs::write(root_path.join("data.txt"), b"anchored")
        .expect("rooted fixture should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let moved_path = root_path.with_extension("moved");
    fs::rename(&root_path, &moved_path).expect("root should be renamed");
    fs::create_dir(&root_path)
        .expect("replacement diagnostic path should exist");
    fs::write(root_path.join("data.txt"), b"replacement")
        .expect("replacement fixture should be written");

    let relative = LocalRelativePath::new("data.txt")
        .expect("reader path should validate");
    let mut reader = root
        .open_reader(&relative, FileReadOptions::unbuffered())
        .expect("anchored reader should open after root rename");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("anchored reader should read");

    assert_eq!("anchored", content);
    fs::remove_dir_all(root_path).expect("replacement root should be removed");
    fs::remove_dir_all(moved_path).expect("moved root should be removed");
}

/// Verifies that an already-open writer remains bound to its original parent
/// descriptor after an intermediate name is replaced by an outside symlink.
#[cfg(unix)]
#[test]
fn test_open_writer_survives_intermediate_directory_replacement() {
    use std::os::unix::fs::symlink;

    let fixture = temp_dir("rooted-writer-parent-replacement");
    let root_path = fixture.join("root");
    let parent_path = root_path.join("parent");
    let moved_parent_path = root_path.join("moved-parent");
    let outside_path = fixture.join("outside");
    fs::create_dir_all(&parent_path).expect("rooted parent should be created");
    fs::create_dir(&outside_path).expect("outside directory should be created");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("parent/data.txt")
        .expect("destination should validate");
    let mut writer = root
        .open_writer(&destination, FileWriteOptions::default())
        .expect("rooted writer should open");
    fs::rename(&parent_path, &moved_parent_path)
        .expect("intermediate parent should be renamed");
    symlink(&outside_path, &parent_path)
        .expect("outside symlink should replace the intermediate name");

    writer
        .write_all(b"anchored")
        .expect("anchored writer should write");
    writer.close().expect("anchored writer should close");

    assert_eq!(
        b"anchored",
        fs::read(moved_parent_path.join("data.txt"))
            .expect("moved destination should be readable")
            .as_slice(),
    );
    assert!(!outside_path.join("data.txt").exists());
    fs::remove_file(parent_path)
        .expect("replacement symlink should be removed");
    fs::remove_dir_all(fixture).expect("writer fixture should be removed");
}

/// Verifies that a final root symbolic link and descendant symbolic links are
/// denied.
#[cfg(unix)]
#[test]
fn test_rooted_io_rejects_symbolic_links() {
    use std::os::unix::fs::symlink;

    let fixture = temp_dir("rooted-symlinks");
    let root_path = fixture.join("root");
    let outside = fixture.join("outside");
    fs::create_dir_all(&root_path).expect("root should be created");
    fs::create_dir_all(&outside).expect("outside directory should be created");
    fs::write(outside.join("secret.txt"), b"secret")
        .expect("outside fixture should be written");
    symlink(&outside, fixture.join("root-link"))
        .expect("root symlink should be created");
    let error = LocalRoot::open(fixture.join("root-link"))
        .expect_err("root symlink should be rejected");
    assert_eq!(ErrorKind::InvalidInput, error.kind());

    symlink(&outside, root_path.join("linked-dir"))
        .expect("intermediate symlink should be created");
    symlink(
        outside.join("secret.txt"),
        root_path.join("linked-file.txt"),
    )
    .expect("final symlink should be created");
    let root = LocalRoot::open(&root_path).expect("real root should open");

    for invalid in ["linked-dir/secret.txt", "linked-file.txt"] {
        let relative = LocalRelativePath::new(invalid)
            .expect("symlink path should be lexically valid");
        let read_error = root
            .open_reader(&relative, FileReadOptions::unbuffered())
            .expect_err("rooted reader should reject symlinks");
        let write_error = root
            .open_writer(&relative, FileWriteOptions::default())
            .expect_err("rooted writer should reject symlinks");
        assert_eq!(ErrorKind::InvalidInput, read_error.kind());
        assert_eq!(ErrorKind::InvalidInput, write_error.kind());
    }

    assert_eq!(
        b"secret",
        fs::read(outside.join("secret.txt"))
            .expect("outside fixture should remain readable")
            .as_slice(),
    );
    fs::remove_dir_all(fixture).expect("symlink fixture should be removed");
}

/// Verifies that root opening resolves symbolic links in ancestor components
/// while rejecting a symbolic link as the final root entry.
#[cfg(unix)]
#[test]
fn test_open_root_allows_symlinked_ancestor() {
    use std::os::unix::fs::symlink;

    let fixture = temp_dir("rooted-ancestor-symlink");
    let real_parent = fixture.join("real-parent");
    let root_path = real_parent.join("root");
    let alias_parent = fixture.join("alias-parent");
    fs::create_dir_all(&root_path)
        .expect("real root directory should be created");
    fs::write(root_path.join("data.txt"), b"anchored")
        .expect("root fixture should be written");
    symlink(&real_parent, &alias_parent)
        .expect("ancestor symbolic link should be created");

    let root = LocalRoot::open(alias_parent.join("root"))
        .expect("ancestor symbolic link should be resolved");
    let path = LocalRelativePath::new("data.txt")
        .expect("relative path should validate");
    let mut reader = root
        .open_reader(&path, FileReadOptions::unbuffered())
        .expect("rooted reader should open through the anchored root");
    let mut content = Vec::new();
    reader
        .read_to_end(&mut content)
        .expect("rooted reader should read through the anchored root");

    assert_eq!(b"anchored", content.as_slice());
    fs::remove_dir_all(fixture)
        .expect("rooted ancestor fixture should be removed");
}

/// Verifies the conservative fallback on targets without a secure rooted
/// backend.
#[cfg(not(unix))]
#[test]
fn test_open_returns_unsupported_without_secure_backend() {
    use std::io::ErrorKind;

    use qubit_local_files::LocalRoot;

    let error = LocalRoot::open(".")
        .expect_err("unsupported platform should reject rooted operations");
    assert_eq!(ErrorKind::Unsupported, error.kind());
}
