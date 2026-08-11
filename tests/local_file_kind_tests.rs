// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Integration coverage for native special-entry classification.

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use qubit_local_files::LocalFileKind;
#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use tempfile::tempdir;

/// Verifies host metadata preserves Unix FIFO and socket kinds.
#[cfg(unix)]
#[test]
fn test_host_metadata_classifies_fifo_and_socket() {
    use std::os::unix::ffi::OsStrExt;

    let directory = tempdir().expect("special-entry directory must be created");
    let fifo = directory.path().join("fifo");
    let fifo_name = CString::new(fifo.as_os_str().as_bytes())
        .expect("FIFO path must not contain an interior NUL");
    // SAFETY: `fifo_name` is a live NUL-terminated path for this call.
    let result = unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) };
    assert_eq!(0, result, "FIFO fixture must be created");
    let socket_path = directory.path().join("socket");
    let _socket = UnixListener::bind(&socket_path).expect("socket fixture must bind");
    let filesystem = LocalFileSystem::host();

    assert_eq!(
        LocalFileKind::Fifo,
        filesystem
            .metadata(&fifo)
            .expect("FIFO metadata must be readable")
            .kind(),
    );
    assert_eq!(
        LocalFileKind::Socket,
        filesystem
            .metadata(&socket_path)
            .expect("socket metadata must be readable")
            .kind(),
    );
}

/// Verifies descriptor-relative metadata uses the same Unix special kinds.
#[cfg(unix)]
#[test]
fn test_rooted_metadata_classifies_fifo_and_socket() {
    use std::os::unix::ffi::OsStrExt;

    let directory = tempdir().expect("rooted special-entry directory must exist");
    let fifo = directory.path().join("fifo");
    let fifo_name = CString::new(fifo.as_os_str().as_bytes())
        .expect("FIFO path must not contain an interior NUL");
    // SAFETY: `fifo_name` is a live NUL-terminated path for this call.
    let result = unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) };
    assert_eq!(0, result, "rooted FIFO fixture must be created");
    let socket_path = directory.path().join("socket");
    let _socket = UnixListener::bind(&socket_path).expect("rooted socket fixture must bind");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("root must open");

    assert_eq!(
        LocalFileKind::Fifo,
        filesystem
            .metadata(Path::new("fifo"))
            .expect("rooted FIFO metadata must be readable")
            .kind(),
    );
    assert_eq!(
        LocalFileKind::Socket,
        filesystem
            .metadata(Path::new("socket"))
            .expect("rooted socket metadata must be readable")
            .kind(),
    );
}

/// Verifies the conventional Unix null device is exposed as a character
/// device instead of being folded into `Other`.
#[cfg(unix)]
#[test]
fn test_host_metadata_classifies_character_device() {
    let path = Path::new("/dev/null");
    let metadata =
        std::fs::symlink_metadata(path).expect("Unix null device must be available for this test");
    assert!(metadata.file_type().is_char_device());

    assert_eq!(
        LocalFileKind::CharDevice,
        LocalFileSystem::host()
            .metadata(path)
            .expect("character-device metadata must be readable")
            .kind(),
    );
}
