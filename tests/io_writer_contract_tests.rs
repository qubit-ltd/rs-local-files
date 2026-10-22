// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;
use std::io::IoSlice;
use std::path::Path;

use qubit_io::Output;
use qubit_io::OutputRef;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use tempfile::tempdir;

#[test]
fn writer_indexed_range_flush_and_owned_adapter_preserve_staging() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("target");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let mut writer = filesystem.open_writer(&path).expect("writer should open");
    // SAFETY: 1..4 is a valid source range.
    let count = unsafe { Output::write_unchecked(&mut writer, b"!abc?", 1, 3) }.expect("range write should work");
    assert_eq!(3, count);
    // SAFETY: 5..5 is a valid empty source range.
    assert_eq!(
        0,
        unsafe { Output::write_unchecked(&mut writer, b"hello", 5, 0) }.unwrap()
    );
    {
        let mut standard = OutputRef::new(&mut writer).into_std_write();
        std::io::Write::write_all(&mut standard, b"d").expect("adapter should append");
        std::io::Write::flush(&mut standard).expect("flush should succeed");
    }
    assert_eq!(2, writer.write_vectored(&[IoSlice::new(b"ef")]).unwrap());
    assert!(!path.exists(), "flush must not publish staged bytes");
    let outcome = writer.commit().expect("commit should publish staged bytes");
    assert_eq!(6, outcome.bytes_written());
    assert_eq!(b"abcdef", std::fs::read(path).unwrap().as_slice());
}

#[test]
fn owned_standard_adapter_returns_writer_for_commit() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("target");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let writer = filesystem.open_writer(&path).expect("writer should open");
    let mut standard = Output::into_std_write(writer);
    std::io::Write::write_all(&mut standard, b"owned").expect("adapter should write");
    let writer = standard.into_inner();
    let outcome = writer.commit().expect("recovered writer should commit");
    assert_eq!(5, outcome.bytes_written());
    assert_eq!(b"owned", std::fs::read(path).unwrap().as_slice());
}

#[test]
fn rooted_writer_uses_virtual_absolute_path_and_publishes_range() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let path = Path::new(std::path::MAIN_SEPARATOR_STR).join("target");
    let mut writer = filesystem.open_writer(&path).expect("rooted writer should open");
    // SAFETY: 2..5 is a valid source range.
    assert_eq!(
        3,
        unsafe { Output::write_unchecked(&mut writer, b"!!xyz!!", 2, 3) }.unwrap()
    );
    let outcome = writer.commit().expect("rooted commit should publish");
    assert_eq!(3, outcome.bytes_written());
    assert_eq!(
        b"xyz",
        std::fs::read(directory.path().join("target")).unwrap().as_slice()
    );
}

#[test]
fn rooted_abort_closes_writer_without_publishing() {
    let directory = tempdir().expect("temporary directory should be created");
    std::fs::write(directory.path().join("target"), b"old").unwrap();
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let path = Path::new(std::path::MAIN_SEPARATOR_STR).join("target");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = filesystem.open_writer_with_options(&path, &options).unwrap();
    Output::write_fully(&mut writer, b"new").unwrap();
    let _outcome = writer.abort().expect("rooted abort should clean staging");
    assert_eq!(
        ErrorKind::BrokenPipe,
        Output::write(&mut writer, b"x").unwrap_err().kind()
    );
    assert_eq!(ErrorKind::BrokenPipe, Output::flush(&mut writer).unwrap_err().kind());
    assert_eq!(
        b"old",
        std::fs::read(directory.path().join("target")).unwrap().as_slice()
    );
}

#[test]
fn append_writer_counts_indexed_range() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("target");
    std::fs::write(&path, b"old").expect("fixture should be written");
    let options = LocalWriteOptions::new(LocalWriteMode::Append);
    let mut writer = LocalFileSystem::host()
        .unwrap()
        .open_writer_with_options(&path, &options)
        .expect("append writer should open");
    // SAFETY: 1..4 is a valid source range.
    assert_eq!(
        3,
        unsafe { Output::write_unchecked(&mut writer, b"!new?", 1, 3) }.unwrap()
    );
    let outcome = writer.commit().expect("append commit should flush");
    assert_eq!(3, outcome.bytes_written());
    assert_eq!(b"oldnew", std::fs::read(path).unwrap().as_slice());
}

#[test]
fn abort_and_drop_keep_staged_destination_unpublished() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("target");
    std::fs::write(&path, b"old").expect("fixture should be written");
    let filesystem = LocalFileSystem::host().unwrap();
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = filesystem.open_writer_with_options(&path, &options).unwrap();
    Output::write_fully(&mut writer, b"new").unwrap();
    let _outcome = writer.abort().expect("abort should clean staging");
    assert_eq!(
        ErrorKind::BrokenPipe,
        Output::write(&mut writer, b"x").unwrap_err().kind()
    );
    assert_eq!(ErrorKind::BrokenPipe, Output::flush(&mut writer).unwrap_err().kind());
    assert_eq!(b"old", std::fs::read(&path).unwrap().as_slice());

    {
        let mut dropped = filesystem.open_writer_with_options(&path, &options).unwrap();
        Output::write_fully(&mut dropped, b"discard").unwrap();
    }
    assert_eq!(b"old", std::fs::read(path).unwrap().as_slice());
}

#[cfg(feature = "test-support")]
#[test]
fn interrupted_and_would_block_remain_retryable_but_fatal_write_is_terminal() {
    use qubit_local_files::test_support::install_test_fault;

    let directory = tempdir().unwrap();
    let path = directory.path().join("target");
    let filesystem = LocalFileSystem::host().unwrap();
    let mut writer = filesystem.open_writer(&path).unwrap();
    for (fault, kind) in [
        ("local-writer-interrupted", ErrorKind::Interrupted),
        ("local-writer-would-block", ErrorKind::WouldBlock),
    ] {
        let _guard = install_test_fault(fault).expect("fault guard should install");
        assert_eq!(kind, Output::write(&mut writer, b"x").unwrap_err().kind());
    }
    Output::write_fully(&mut writer, b"ok").expect("retry should succeed");
    let _outcome = writer.commit().expect("retryable writer should commit");

    let fatal_path = directory.path().join("fatal");
    let mut fatal = filesystem.open_writer(&fatal_path).unwrap();
    {
        let _guard = install_test_fault("local-writer-write-failed").expect("fault guard should install");
        assert_eq!(ErrorKind::Other, Output::write(&mut fatal, b"x").unwrap_err().kind());
    }
    assert_eq!(
        ErrorKind::BrokenPipe,
        Output::write(&mut fatal, b"x").unwrap_err().kind()
    );
    assert_eq!(ErrorKind::BrokenPipe, Output::flush(&mut fatal).unwrap_err().kind());
    assert!(fatal.commit().is_err());
}
