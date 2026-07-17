// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative atomic-write preparation.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::ffi::CString;
use std::fs::{
    File,
    Permissions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::LocalFilenames;

use super::rooted_file_io::open_file_at;
use super::rooted_io_result::missing_rooted_file_permissions;
use super::rooted_staged_file::RootedStagedFile;
use super::rooted_staging_retry::retry_rooted_staging_entry;
use super::temp_entry::DEFAULT_TEMP_FILE_RETRIES;

/// Prefix used by descriptor-relative atomic staging entries.
const ROOTED_ATOMIC_TEMP_PREFIX: &str = ".atomic-write-";

/// Suffix used by descriptor-relative atomic staging entries.
const ROOTED_ATOMIC_TEMP_SUFFIX: &str = ".tmp";

/// Reads existing regular-file permissions without following the final entry.
///
/// # Parameters
///
/// * `parent` - Open destination parent descriptor.
/// * `name` - Final destination entry name.
///
/// # Returns
///
/// Existing ordinary-file permissions, or `None` when the entry is missing.
///
/// # Errors
///
/// Returns `InvalidInput` when the entry is a link or non-regular resource,
/// and otherwise returns the operating-system error from `fstatat`.
pub(in crate::local) fn existing_rooted_file_permissions(
    parent: &File,
    name: &CString,
) -> Result<Option<Permissions>> {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `status` is writable storage and the live parent and name remain
    // valid for this non-retaining metadata operation.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        return missing_rooted_file_permissions(Error::last_os_error());
    }
    // SAFETY: successful `fstatat` initialized the complete status value.
    let status = unsafe { status.assume_init() };
    if status.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "rooted atomic destination is not a regular file",
        ));
    }
    // `mode_t` varies across Unix targets. Permission construction uses the
    // portable `u32` representation exposed by `PermissionsExt`.
    #[allow(clippy::useless_conversion)]
    let mode = u32::from(status.st_mode);
    Ok(Some(Permissions::from_mode(mode)))
}

/// Creates a unique staging entry in an open destination parent directory.
///
/// # Parameters
///
/// * `parent` - The open destination-parent handle transferred to the staging
///   guard.
/// * `relative_parent` - The diagnostic parent path used to identify the
///   staging entry.
///
/// # Returns
///
/// An armed descriptor-relative staging guard.
///
/// # Errors
///
/// Returns an I/O error when randomness fails, all generated names collide, or
/// `openat` cannot create a private staging entry.
///
/// # Panics
///
/// Panics if the filename generator violates its no-NUL invariant.
pub(in crate::local) fn create_rooted_staged_file(
    parent: File,
    relative_parent: &Path,
) -> Result<RootedStagedFile> {
    retry_rooted_staging_entry(
        DEFAULT_TEMP_FILE_RETRIES,
        || {
            LocalFilenames::try_random_with(
                Some(ROOTED_ATOMIC_TEMP_PREFIX),
                Some(ROOTED_ATOMIC_TEMP_SUFFIX),
            )
        },
        |name| {
            let native_name = CString::new(name.as_bytes()).expect(
                "LocalFilenames guarantees generated names without NUL",
            );
            open_file_at(
                &parent,
                &native_name,
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
                0o600,
            )
            .map(|file| (native_name, file))
        },
    )
    .map(|(name, native_name, file)| {
        RootedStagedFile::new(
            parent,
            native_name,
            file,
            relative_parent.join(name),
        )
    })
}
