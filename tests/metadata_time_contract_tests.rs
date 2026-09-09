// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Public timestamp contracts shared by Host and Rooted operations.

use std::fs::File;
use std::fs::FileTimes;
use std::path::Path;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;

/// Compares pathname, opened-handle and walker timestamps with native readback.
#[test]
fn test_metadata_preserves_pre_epoch_timestamps() {
    let directory = tempdir().expect("timestamp fixture should be created");
    let native_path = directory.path().join("entry");
    let file = File::create(&native_path).expect("fixture file should be created");
    let host = LocalFileSystem::host().expect("Host filesystem should open");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    for duration in [Duration::from_secs(1), Duration::from_millis(500), Duration::ZERO] {
        let requested = UNIX_EPOCH.checked_sub(duration).expect("timestamp is representable");
        file.set_times(FileTimes::new().set_modified(requested).set_accessed(requested))
            .expect("fixture timestamps should be set");
        let native = file.metadata().expect("native metadata should be readable");
        #[cfg(target_os = "macos")]
        diagnose_native_times(&native_path);
        for (filesystem, path, parent) in [
            (&host, native_path.as_path(), directory.path()),
            (&rooted, Path::new("/entry"), Path::new("/")),
        ] {
            let metadata = filesystem.metadata(path).expect("entry metadata should be readable");
            assert_eq!(
                metadata.modified_at(),
                native.modified().ok(),
                "namespace operand: {path:?}"
            );
            assert_eq!(metadata.accessed_at(), native.accessed().ok());
            let reader = filesystem.open_reader(path).expect("reader should open");
            assert_eq!(reader.metadata().modified_at(), native.modified().ok());
            let entries = filesystem
                .list(parent)
                .expect("walker should open")
                .collect::<Result<Vec<_>, _>>()
                .expect("walker should finish");
            let entry = entries
                .iter()
                .find(|entry| entry.path().file_name() == path.file_name())
                .expect("walker should contain the fixture");
            assert_eq!(entry.metadata().modified_at(), native.modified().ok());
        }
    }
}

/// Prints direct pathname stat timestamps to diagnose platform interceptor
/// mismatches.
#[cfg(target_os = "macos")]
fn diagnose_native_times(path: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(path.as_os_str().as_bytes()).expect("native pathname");
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the pathname is NUL-terminated and status provides writable stat
    // storage.
    let result = unsafe {
        libc::fstatat(
            libc::AT_FDCWD,
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    assert_eq!(result, 0, "diagnostic stat must succeed");
    // SAFETY: successful fstatat initialized status.
    let status = unsafe { status.assume_init() };
    eprintln!(
        "native fstatat: mtime={}.{}, atime={}.{}",
        status.st_mtime, status.st_mtime_nsec, status.st_atime, status.st_atime_nsec
    );
}
