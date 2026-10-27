// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSlice;
use std::io::IoSliceMut;
use std::path::Path;

use qubit_io::Input;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
#[cfg(all(unix, feature = "test-support"))]
use qubit_local_files::test_support::install_test_fault;

#[test]
fn generic_vectored_reader_fills_multiple_buffers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"abcdef").unwrap();
    let fs = LocalFileSystem::host().unwrap();
    let mut reader = fs.open_reader(&path).unwrap();
    let mut a = [0; 2];
    let mut b = [0; 4];
    assert_eq!(
        Input::read_vectored(&mut reader, &mut [&mut [], &mut a, &mut b]).unwrap(),
        6
    );
    assert_eq!(&a, b"ab");
    assert_eq!(&b, b"cdef");
}

#[test]
fn generic_vectored_writer_counts_once_for_all_backends() {
    let dir = tempfile::tempdir().unwrap();
    let host = LocalFileSystem::host().unwrap();
    let rooted = LocalFileSystem::rooted(dir.path()).unwrap();
    let host_path = dir.path().join("host");
    let root_path = Path::new(std::path::MAIN_SEPARATOR_STR).join("rooted");
    let append_path = dir.path().join("append");
    std::fs::write(&append_path, b"old").unwrap();
    let options = LocalWriteOptions::new(LocalWriteMode::Append);
    let writers = [
        host.open_writer(&host_path).unwrap(),
        rooted.open_writer(&root_path).unwrap(),
        host.open_writer_with_options(&append_path, &options).unwrap(),
    ];
    for mut writer in writers {
        let n = Output::write_vectored(&mut writer, &[b"", b"ab", b"cd"]).unwrap();
        assert!(n > 0 && n <= 4);
        assert_eq!(writer.commit().unwrap().bytes_written(), n);
    }
    for (path, prefix) in [
        (host_path, b"".as_slice()),
        (dir.path().join("rooted"), b"".as_slice()),
        (append_path, b"old".as_slice()),
    ] {
        let content = std::fs::read(path).unwrap();
        assert!(content.starts_with(prefix));
        let tail = &content[prefix.len()..];
        assert!(!tail.is_empty());
        assert_eq!(tail, &b"abcd"[..tail.len()]);
    }
}

#[test]
fn vectored_writer_entries_share_the_byte_count() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFileSystem::host().unwrap();
    let path = dir.path().join("mixed");
    let mut writer = fs.open_writer(&path).unwrap();
    let first = Output::write_vectored(&mut writer, &[b"a", b"b"]).unwrap();
    let second = writer
        .write_vectored(&[IoSlice::new(b"c"), IoSlice::new(b"d")])
        .unwrap();
    assert_eq!(writer.commit().unwrap().bytes_written(), first + second);
    let mut expected = Vec::new();
    expected.extend_from_slice(&b"ab"[..first]);
    expected.extend_from_slice(&b"cd"[..second]);
    assert_eq!(std::fs::read(path).unwrap(), expected);
}

#[test]
fn closed_temp_file_checks_state_before_empty_vectored_write() {
    let fs = LocalFileSystem::host().unwrap();
    let mut temporary = fs.create_temp_file().unwrap();
    temporary.close();
    assert_eq!(
        Output::write_vectored(&mut temporary, &[]).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        temporary.write_vectored(&[]).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
}

#[test]
fn aborted_writer_checks_state_before_empty_vectored_write() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFileSystem::host().unwrap();
    let mut writer = fs.open_writer(&dir.path().join("aborted")).unwrap();
    let _outcome = writer.abort().unwrap();
    assert_eq!(
        Output::write_vectored(&mut writer, &[]).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        writer.write_vectored(&[]).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
}

#[test]
fn active_writer_accepts_empty_vectored_write_without_counting_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFileSystem::host().unwrap();
    let mut writer = fs.open_writer(&dir.path().join("empty")).unwrap();
    assert_eq!(Output::write(&mut writer, b"a").unwrap(), 1);
    assert_eq!(Output::write_vectored(&mut writer, &[]).unwrap(), 0);
    assert_eq!(writer.commit().unwrap().bytes_written(), 1);
}

#[cfg(all(feature = "test-support", unix))]
#[test]
fn failed_writer_checks_failure_state_before_empty_vectored_write() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFileSystem::host().unwrap();
    let mut writer = fs.open_writer(&dir.path().join("failed")).unwrap();
    let _fault = install_test_fault("local-writer-write-failed").unwrap();
    assert!(writer.write(b"payload").is_err());
    assert_eq!(
        Output::write_vectored(&mut writer, &[]).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
}

#[test]
fn generic_vectored_reader_keeps_the_legacy_entry_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    std::fs::write(&path, b"abcdef").unwrap();
    let fs = LocalFileSystem::host().unwrap();
    let mut reader = fs.open_reader(&path).unwrap();
    let mut a = [0; 2];
    let mut b = [0; 4];
    let mut buffers = [IoSliceMut::new(&mut a), IoSliceMut::new(&mut b)];
    assert_eq!(reader.read_vectored(&mut buffers).unwrap(), 6);
    assert_eq!(&a, b"ab");
    assert_eq!(&b, b"cdef");
}
