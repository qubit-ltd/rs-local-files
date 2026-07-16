// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Write;

use qubit_local_files::{
    LocalAtomicWriter,
    LocalFiles,
};

use super::test_support::{
    count_atomic_temp_files,
    fs,
    temp_dir,
};

fn assert_send<T: Send>() {}

#[test]
fn test_local_atomic_writer_is_send() {
    assert_send::<LocalAtomicWriter>();
}

#[test]
fn test_local_atomic_writer_commits_written_contents() {
    let dir = temp_dir("atomic-writer-commit");
    let path = dir.join("out.txt");
    let mut writer = LocalFiles::begin_atomic_write(&path)
        .expect("atomic writer should begin");
    writer
        .write_all(b"committed")
        .expect("contents should write");
    assert!(!path.exists());
    writer.commit().expect("atomic writer should commit");
    assert_eq!(b"committed", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_abort_preserves_destination() {
    let dir = temp_dir("atomic-writer-abort");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").unwrap();
    let mut writer = LocalFiles::begin_atomic_write(&path).unwrap();
    writer.write_all(b"replacement").unwrap();
    writer.abort().expect("atomic writer should abort");
    assert_eq!(b"original", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_drop_removes_staging_file() {
    let dir = temp_dir("atomic-writer-drop");
    let path = dir.join("out.txt");
    {
        let mut writer = LocalFiles::begin_atomic_write(&path).unwrap();
        writer.write_all(b"discarded").unwrap();
    }
    assert!(!path.exists());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}
