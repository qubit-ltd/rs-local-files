// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSlice;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;

use qubit_io::Output;
use qubit_io::OutputRef;
use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;

fn rooted_host_path(root: &Path, virtual_path: &Path) -> std::path::PathBuf {
    root.join(
        virtual_path
            .strip_prefix(Path::new(std::path::MAIN_SEPARATOR_STR))
            .expect("rooted paths are virtual absolute paths"),
    )
}

#[test]
fn temp_adapter_seek_and_cleanup_preserve_ownership() {
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let mut file = fs.create_temp_file().expect("temporary file should be created");

    // SAFETY: 1..4 is a valid source range.
    let count = unsafe { Output::write_unchecked(&mut file, b"!abc?", 1, 3) }.expect("indexed write should succeed");
    assert_eq!(count, 3);
    Output::write_fully(&mut file, &b"abc"[count..]).expect("remaining bytes should be written");
    // SAFETY: 0..0 is a valid empty source range.
    assert_eq!(unsafe { Output::write_unchecked(&mut file, b"x", 0, 0) }.unwrap(), 0);

    {
        let mut std_file = OutputRef::new(&mut file).into_std_write();
        std::io::Seek::seek(&mut std_file, SeekFrom::Start(0)).expect("adapter should seek");
        std::io::Write::write_all(&mut std_file, b"Z").expect("adapter should overwrite first byte");
    }
    assert_eq!(file.stream_position().expect("temp file should report position"), 1);

    let native_file = file.as_file_mut().expect("native handle should remain open");
    native_file.seek(SeekFrom::Start(0)).expect("native file should seek");
    let mut bytes = [0; 3];
    native_file.read_exact(&mut bytes).expect("native file should read");
    assert_eq!(&bytes, b"Zbc");

    file.close();
    assert_eq!(
        Output::write(&mut file, b"x").unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        Output::flush(&mut file).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        file.write_vectored(&[IoSlice::new(b"x")]).unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    file.cleanup().expect("host temporary resource should clean up");
}

#[test]
fn temp_vectored_write_uses_inherent_method() {
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let mut file = fs.create_temp_file().expect("temporary file should be created");
    let written = file
        .write_vectored(&[IoSlice::new(b"vec"), IoSlice::new(b"tored")])
        .expect("vectored write should succeed");
    assert_eq!(written, 8);
    file.as_file_mut()
        .expect("native handle should remain open")
        .seek(SeekFrom::Start(0))
        .expect("native file should seek");
    let mut contents = [0; 8];
    file.as_file_mut()
        .expect("native handle should remain open")
        .read_exact(&mut contents)
        .expect("native file should read vectored contents");
    assert_eq!(&contents, b"vectored");
    file.cleanup().expect("temporary resource should clean up");
}

#[test]
fn rooted_temp_range_write_and_cleanup_preserve_authority() {
    let directory = tempdir().expect("root directory should be created");
    let fs = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let mut file = fs.create_temp_file().expect("rooted temporary file should be created");
    let public_path = file.path().to_path_buf();
    assert!(!Output::is_buffered(&file));

    // SAFETY: 2..5 is a valid source range.
    let count =
        unsafe { Output::write_unchecked(&mut file, b"!!xyz!!", 2, 3) }.expect("rooted indexed write should succeed");
    assert_eq!(count, 3);
    Output::write_fully(&mut file, &b"xyz"[count..]).expect("remaining bytes should be written");
    assert_eq!(
        file.stream_position().expect("rooted temp file should report position"),
        3
    );
    let native_path = rooted_host_path(directory.path(), &public_path);
    assert_eq!(
        std::fs::read(&native_path).expect("rooted temp file should be readable"),
        b"xyz"
    );

    file.cleanup().expect("rooted temporary resource should clean up");
    assert!(!rooted_host_path(directory.path(), &public_path).exists());
}

#[test]
fn owned_std_write_can_be_recovered_before_keep_and_persist() {
    let directory = tempfile::tempdir_in(".").expect("same-filesystem destination directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");

    let mut keep_file = fs.create_temp_file().expect("temporary file should be created");
    Output::write_fully(&mut keep_file, b"kept").expect("keep contents should be written");
    let std_file = keep_file.into_std_write().into_inner();
    let outcome = std_file.keep().expect("recovered temp file should be kept");
    assert_eq!(
        std::fs::read(outcome.path()).expect("kept file should be readable"),
        b"kept"
    );
    std::fs::remove_file(outcome.path()).expect("kept test artifact should be removed");

    let mut persist_file = fs.create_temp_file().expect("second temporary file should be created");
    Output::write_fully(&mut persist_file, b"persisted").expect("persist contents should be written");
    let persist_target = directory.path().join("persisted.txt");
    let std_file = persist_file.into_std_write().into_inner();
    let _outcome = std_file
        .persist(&persist_target)
        .expect("recovered temp file should be persisted");
    assert_eq!(
        std::fs::read(persist_target).expect("persisted file should be readable"),
        b"persisted"
    );
}
