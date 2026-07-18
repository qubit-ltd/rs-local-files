// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::{
    Seek,
    SeekFrom,
    Write,
};

#[cfg(unix)]
use qubit_local_files::FileWriteMode;
use qubit_local_files::{
    FileWriteOptions,
    LocalFiles,
};

#[cfg(target_os = "linux")]
use super::test_support::file_status_flags;
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
