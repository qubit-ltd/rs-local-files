// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::{
    ErrorKind,
    IoSliceMut,
    Read,
    Seek,
    SeekFrom,
};

use qubit_local_files::{
    FileReadOptions,
    LocalFiles,
};

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
fn test_open_reader_respects_buffering_options_and_rejects_directories() {
    let dir = temp_dir("open-reader-options");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").expect("reader fixture should be written");

    let mut reader = LocalFiles::open_reader(
        &path,
        FileReadOptions::buffered_with_capacity(16)
            .expect("positive buffer capacity should be accepted"),
    )
    .expect("buffered reader should open");
    let mut content = Vec::new();
    reader
        .read_to_end(&mut content)
        .expect("buffered reader should read the fixture");

    let error = LocalFiles::open_reader(&dir, FileReadOptions::default())
        .expect_err("directories should not be accepted as files");

    assert!(reader.is_buffered());
    assert_eq!(b"payload", content.as_slice());
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(
        error.to_string().contains("path is not a regular file"),
        "reader validation must describe the rejected resource: {error}"
    );
    fs::remove_dir_all(dir).expect("reader fixture should be removed");
}

#[test]
fn test_local_file_reader_supports_seek_for_unbuffered_and_buffered_readers() {
    let dir = temp_dir("reader-seek");
    let path = dir.join("data.txt");
    fs::write(&path, b"abcdef").unwrap();

    let mut unbuffered =
        LocalFiles::open_reader(&path, FileReadOptions::unbuffered())
            .expect("unbuffered reader should open");
    let mut unbuffered_bytes = [0; 2];
    assert_eq!(
        2,
        unbuffered
            .seek(SeekFrom::Start(2))
            .expect("unbuffered reader should seek")
    );
    unbuffered
        .read_exact(&mut unbuffered_bytes)
        .expect("unbuffered reader should read from seeked position");

    let mut buffered = LocalFiles::open_reader(
        &path,
        FileReadOptions::buffered_with_capacity(4)
            .expect("positive buffer capacity should be accepted"),
    )
    .expect("buffered reader should open");
    let mut first = [0; 1];
    let mut buffered_tail = Vec::new();
    buffered
        .read_exact(&mut first)
        .expect("buffered reader should fill its internal buffer");
    assert_eq!(
        4,
        buffered
            .seek(SeekFrom::Start(4))
            .expect("buffered reader should seek")
    );
    buffered
        .read_to_end(&mut buffered_tail)
        .expect("buffered reader should read after seek");

    assert_eq!(b"cd", &unbuffered_bytes);
    assert_eq!(b"a", &first);
    assert_eq!(b"ef", buffered_tail.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_file_reader_forwards_vectored_reads_to_buffered_reader() {
    let dir = temp_dir("reader-vectored");
    let path = dir.join("data.txt");
    fs::write(&path, b"abcdefghi").expect("reader fixture should be written");
    let mut reader = LocalFiles::open_reader(
        &path,
        FileReadOptions::buffered_with_capacity(8)
            .expect("positive buffer capacity should be accepted"),
    )
    .expect("buffered reader should open");
    let mut prefix = [0; 1];
    reader
        .read_exact(&mut prefix)
        .expect("initial read should fill the internal buffer");
    let mut first = [0; 2];
    let mut second = [0; 2];
    let mut buffers =
        [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count = reader
        .read_vectored(&mut buffers)
        .expect("vectored read should succeed");

    assert_eq!(4, count);
    assert_eq!(b"a", &prefix);
    assert_eq!(b"bc", &first);
    assert_eq!(b"de", &second);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_local_file_reader_forwards_vectored_reads_to_unbuffered_file() {
    let dir = temp_dir("reader-unbuffered-vectored");
    let path = dir.join("data.txt");
    fs::write(&path, b"abcd").expect("reader fixture should be written");
    let mut reader =
        LocalFiles::open_reader(&path, FileReadOptions::unbuffered())
            .expect("unbuffered reader should open");
    let mut first = [0; 2];
    let mut second = [0; 2];
    let mut buffers =
        [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count = reader
        .read_vectored(&mut buffers)
        .expect("vectored read should succeed");

    assert_eq!(4, count);
    assert_eq!(b"ab", &first);
    assert_eq!(b"cd", &second);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_open_reader_rejects_fifo_without_blocking() {
    let dir = temp_dir("open-reader-fifo");
    let fifo = dir.join("input.fifo");
    create_fifo(&fifo);

    assert_fifo_open_is_rejected(fifo, |path| {
        LocalFiles::open_reader(path, FileReadOptions::unbuffered()).map(|_| ())
    });

    fs::remove_dir_all(dir).expect("reader FIFO fixture should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_open_reader_clears_transient_nonblocking_status() {
    let dir = temp_dir("open-reader-blocking-status");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").expect("reader fixture should be written");

    let reader = LocalFiles::open_reader(&path, FileReadOptions::unbuffered())
        .expect("reader should open");

    assert_eq!(
        0,
        file_status_flags(&path) & libc::O_NONBLOCK,
        "anti-FIFO-race flags must not leak into the returned reader",
    );
    drop(reader);
    fs::remove_dir_all(dir).expect("reader fixture should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_open_reader_waits_for_conflicting_file_lease() {
    let dir = temp_dir("open-reader-file-lease");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").expect("reader fixture should be written");
    let lease = SourceReadLease::acquire(&path)
        .expect("write lease should be acquired");
    let worker_path = path.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        sender
            .send(LocalFiles::open_reader(
                worker_path,
                FileReadOptions::unbuffered(),
            ))
            .expect("reader result should be sent");
    });

    lease
        .wait_for_break()
        .expect("reader open should request a lease break");
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
            panic!("reader open returned before lease release: {result:?}");
        }
    };
    worker.join().expect("reader worker should not panic");
    let reader = result.expect("reader should open after lease release");

    drop(reader);
    fs::remove_dir_all(dir).expect("reader fixture should be removed");
}
