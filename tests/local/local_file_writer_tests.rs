// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::{
    IoSlice,
    Seek,
    SeekFrom,
    Write,
};

#[cfg(target_os = "linux")]
use std::io::ErrorKind;

#[cfg(unix)]
use qubit_local_files::FileWriteMode;
use qubit_local_files::{
    FileWriteOptions,
    LocalFiles,
};

#[cfg(all(coverage, target_os = "linux"))]
use super::test_support::run_in_coverage_fault_process;
#[cfg(target_os = "linux")]
use super::test_support::{
    SourceReadLease,
    file_status_flags,
};
#[cfg(unix)]
use super::test_support::{
    assert_fifo_open_is_rejected,
    create_fifo,
};
use super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_local_file_writer_supports_seek_for_unbuffered_and_buffered_writers() {
    let dir = temp_dir("writer-seek");
    let unbuffered_path = dir.join("unbuffered.txt");
    let buffered_path = dir.join("buffered.txt");

    {
        let mut writer = LocalFiles::open_writer(
            &unbuffered_path,
            FileWriteOptions::default(),
        )
        .expect("unbuffered writer should open");
        writer.write_all(b"abcdef").unwrap();
        assert_eq!(
            2,
            writer
                .seek(SeekFrom::Start(2))
                .expect("unbuffered writer should seek")
        );
        writer.write_all(b"XY").unwrap();
        writer.close().unwrap();
    }

    {
        let mut writer = LocalFiles::open_writer(
            &buffered_path,
            FileWriteOptions::default()
                .buffered_with_capacity(4)
                .expect("positive buffer capacity should be accepted"),
        )
        .expect("buffered writer should open");
        writer.write_all(b"abcdef").unwrap();
        assert_eq!(
            2,
            writer
                .seek(SeekFrom::Start(2))
                .expect("buffered writer should seek and flush")
        );
        writer.write_all(b"XY").unwrap();
        writer.close().unwrap();
    }

    assert_eq!(b"abXYef", fs::read(&unbuffered_path).unwrap().as_slice());
    assert_eq!(b"abXYef", fs::read(&buffered_path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_file_writer_forwards_vectored_writes_to_buffered_writer() {
    let dir = temp_dir("writer-vectored");
    let path = dir.join("data.txt");
    let mut writer = LocalFiles::open_writer(
        &path,
        FileWriteOptions::default()
            .buffered_with_capacity(16)
            .expect("positive buffer capacity should be accepted"),
    )
    .expect("buffered writer should open");
    let buffers = [IoSlice::new(b"ab"), IoSlice::new(b"cd")];

    let count = writer
        .write_vectored(&buffers)
        .expect("vectored write should succeed");
    writer.close().expect("buffered writer should close");

    assert_eq!(4, count);
    assert_eq!(b"abcd", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_local_file_writer_forwards_vectored_writes_to_unbuffered_file() {
    let dir = temp_dir("writer-unbuffered-vectored");
    let path = dir.join("data.txt");
    let mut writer =
        LocalFiles::open_writer(&path, FileWriteOptions::default())
            .expect("unbuffered writer should open");
    let buffers = [IoSlice::new(b"ab"), IoSlice::new(b"cd")];

    let count = writer
        .write_vectored(&buffers)
        .expect("vectored write should succeed");
    writer.close().expect("unbuffered writer should close");

    assert_eq!(4, count);
    assert_eq!(b"abcd", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_file_writer_sync_methods_flush_buffered_contents() {
    let dir = temp_dir("writer-sync");
    let path = dir.join("data.txt");

    let mut writer = LocalFiles::open_writer(
        &path,
        FileWriteOptions::default()
            .buffered_with_capacity(32)
            .expect("positive buffer capacity should be accepted"),
    )
    .expect("buffered writer should open");
    writer.write_all(b"sync-all").unwrap();
    writer
        .sync_all()
        .expect("sync_all should flush and sync buffered contents");
    assert_eq!(b"sync-all", fs::read(&path).unwrap().as_slice());

    writer.write_all(b"-sync-data").unwrap();
    writer
        .sync_data()
        .expect("sync_data should flush and sync buffered contents");
    assert_eq!(b"sync-all-sync-data", fs::read(&path).unwrap().as_slice());
    writer.close().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_file_writer_sync_methods_support_unbuffered_files() {
    let dir = temp_dir("unbuffered-writer-sync");
    let path = dir.join("data.txt");
    let mut writer =
        LocalFiles::open_writer(&path, FileWriteOptions::default())
            .expect("unbuffered writer should open");

    writer.write_all(b"sync-all").unwrap();
    writer
        .sync_all()
        .expect("sync_all should support unbuffered files");
    writer.write_all(b"-sync-data").unwrap();
    writer
        .sync_data()
        .expect("sync_data should support unbuffered files");
    writer.close().unwrap();

    assert_eq!(b"sync-all-sync-data", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

/// Verifies that opened-handle validation fails before destructive truncation.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_open_writer_preserves_contents_when_handle_validation_fails() {
    const TEST_NAME: &str = concat!(
        "local::local_file_writer_tests::",
        "test_open_writer_preserves_contents_when_handle_validation_fails",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "file-handle-metadata",
        || {
            let dir = temp_dir("file-handle-metadata");
            let path = dir.join("data.txt");
            fs::write(&path, b"original")
                .expect("writer fixture should be written");
            let error =
                LocalFiles::open_writer(&path, FileWriteOptions::default())
                    .expect_err("injected handle validation should fail");

            assert_eq!(ErrorKind::Other, error.kind());
            assert!(
                error.to_string().contains("inspect opened file writer"),
                "validation error should retain operation context: {error}",
            );
            assert_eq!(
                b"original",
                fs::read(&path)
                    .expect("failed writer destination should remain readable")
                    .as_slice(),
            );
            fs::remove_dir_all(dir).expect("writer fixture should be removed");
        },
    ) else {
        return;
    };
}

#[cfg(unix)]
#[test]
fn test_open_writer_rejects_fifo_without_blocking() {
    let dir = temp_dir("open-writer-fifo");
    let fifo = dir.join("output.fifo");
    create_fifo(&fifo);

    assert_fifo_open_is_rejected(fifo, |path| {
        LocalFiles::open_writer(
            path,
            FileWriteOptions::new(FileWriteMode::OpenExistingAtStart),
        )
        .map(|_| ())
    });

    fs::remove_dir_all(dir).expect("writer FIFO fixture should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_open_writer_clears_transient_nonblocking_status() {
    let dir = temp_dir("open-writer-blocking-status");
    let path = dir.join("data.txt");

    let writer = LocalFiles::open_writer(&path, FileWriteOptions::default())
        .expect("writer should open");

    assert_eq!(
        0,
        file_status_flags(&path) & libc::O_NONBLOCK,
        "anti-FIFO-race flags must not leak into the returned writer",
    );
    drop(writer);
    fs::remove_dir_all(dir).expect("writer fixture should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_open_writer_waits_for_conflicting_file_lease() {
    let dir = temp_dir("open-writer-file-lease");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").expect("writer fixture should be written");
    let lease = SourceReadLease::acquire(&path)
        .expect("write lease should be acquired");
    let worker_path = path.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        sender
            .send(LocalFiles::open_writer(
                worker_path,
                FileWriteOptions::new(FileWriteMode::OpenExistingAtStart),
            ))
            .expect("writer result should be sent");
    });

    lease
        .wait_for_break()
        .expect("writer open should request a lease break");
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
            panic!("writer open returned before lease release: {result:?}");
        }
    };
    worker.join().expect("writer worker should not panic");
    let writer = result.expect("writer should open after lease release");

    drop(writer);
    fs::remove_dir_all(dir).expect("writer fixture should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_open_writer_timeout_reports_lease_conflict() {
    let dir = temp_dir("open-writer-lease-timeout");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").expect("writer fixture should be written");
    let lease = SourceReadLease::acquire(&path)
        .expect("write lease should be acquired");
    let worker_path = path.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        sender
            .send(LocalFiles::open_writer(
                worker_path,
                FileWriteOptions::new(FileWriteMode::OpenExistingAtStart)
                    .with_open_retry_timeout(std::time::Duration::ZERO),
            ))
            .expect("writer result should be sent");
    });

    lease
        .wait_for_break()
        .expect("writer open should request a lease break");
    let error = receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("timed writer result should arrive")
        .expect_err("zero timeout should reject the lease conflict");
    assert_eq!(ErrorKind::TimedOut, error.kind());
    lease.release().expect("write lease should be released");
    worker.join().expect("writer worker should not panic");
    fs::remove_dir_all(dir).expect("writer fixture should be removed");
}
