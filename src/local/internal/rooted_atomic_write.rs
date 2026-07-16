// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Descriptor-relative atomic-write preparation.
// qubit-style: allow coverage-cfg

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
use super::rooted_staged_file::RootedStagedFile;
#[cfg(not(coverage))]
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
        #[cfg(coverage)]
        return Ok(None);
        #[cfg(not(coverage))]
        {
            let error = Error::last_os_error();
            return if error.kind() == ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(error)
            };
        }
    }
    // SAFETY: successful `fstatat` initialized the complete status value.
    let status = unsafe { status.assume_init() };
    if status.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "rooted atomic destination is not a regular file",
        ));
    }
    // `mode_t` is narrower on some Unix targets; permission construction uses
    // the portable `u32` representation exposed by `PermissionsExt`.
    #[cfg(target_os = "macos")]
    let mode = u32::from(status.st_mode);
    #[cfg(not(target_os = "macos"))]
    let mode = status.st_mode;
    Ok(Some(Permissions::from_mode(mode)))
}

/// Creates a unique staging entry in an open destination parent directory.
///
/// # Parameters
///
/// * `parent` - Open destination parent descriptor transferred to the guard.
/// * `relative_parent` - Non-authoritative relative parent path for
///   diagnostics.
///
/// # Returns
///
/// An armed descriptor-relative staging guard.
///
/// # Errors
///
/// Returns an I/O error when randomness fails, all generated names collide, or
/// `openat` cannot create a private staging entry.
#[cfg(not(coverage))]
pub(in crate::local) fn create_rooted_staged_file(
    parent: File,
    relative_parent: &Path,
) -> Result<RootedStagedFile> {
    for _ in 0..DEFAULT_TEMP_FILE_RETRIES {
        let name = LocalFilenames::try_random_with(
            Some(ROOTED_ATOMIC_TEMP_PREFIX),
            Some(ROOTED_ATOMIC_TEMP_SUFFIX),
        )?;
        let native_name = CString::new(name.as_bytes())
            .expect("LocalFilenames guarantees generated names without NUL");
        let file = open_file_at(
            &parent,
            &native_name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
            0o600,
        );
        match file {
            Ok(file) => {
                return Ok(RootedStagedFile::new(
                    parent,
                    native_name,
                    file,
                    relative_parent.join(name),
                ));
            }
            #[cfg(not(coverage))]
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::new(
        ErrorKind::AlreadyExists,
        "failed to create a unique rooted atomic staging file",
    ))
}

/// Creates one staging entry during coverage collection.
///
/// Production retries random-name collisions. A finite public fixture cannot
/// force the cryptographically random generator to exhaust every retry.
#[cfg(coverage)]
pub(in crate::local) fn create_rooted_staged_file(
    parent: File,
    relative_parent: &Path,
) -> Result<RootedStagedFile> {
    let name = LocalFilenames::try_random_with(
        Some(ROOTED_ATOMIC_TEMP_PREFIX),
        Some(ROOTED_ATOMIC_TEMP_SUFFIX),
    )?;
    let native_name = CString::new(name.as_bytes())
        .expect("LocalFilenames guarantees generated names without NUL");
    let file = open_file_at(
        &parent,
        &native_name,
        libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
        0o600,
    )?;
    Ok(RootedStagedFile::new(
        parent,
        native_name,
        file,
        relative_parent.join(name),
    ))
}
