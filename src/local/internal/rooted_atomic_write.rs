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
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::os::fd::AsRawFd;
use std::path::Path;

use super::rooted_file_io::open_file_at;
use super::rooted_staged_file::RootedStagedFile;
use super::rooted_staging_retry::retry_rooted_staging_entry;
use super::temp_entry::DEFAULT_TEMP_ENTRY_RETRIES;
use super::unix_stat::is_regular_file_mode;
use crate::local::try_random_file_name;

/// Prefix used by descriptor-relative atomic staging entries.
const ROOTED_ATOMIC_TEMP_PREFIX: &str = ".atomic-write-";

/// Suffix used by descriptor-relative atomic staging entries.
const ROOTED_ATOMIC_TEMP_SUFFIX: &str = ".tmp";

/// Validates an existing final entry without following it.
///
/// # Parameters
///
/// * `parent` - Open destination parent descriptor.
/// * `name` - Final destination entry name.
///
/// # Returns
///
/// A pair containing destination existence and whether regular-file metadata
/// must be preserved.
///
/// # Errors
///
/// Returns `InvalidInput` for a non-regular resource or a symbolic link when
/// link-entry replacement is disabled.
pub(in crate::local) fn inspect_rooted_atomic_destination(
    parent: &File,
    name: &CString,
    replace_target_symlink: bool,
) -> Result<(bool, bool)> {
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
        let error = Error::last_os_error();
        return if error.kind() == ErrorKind::NotFound {
            Ok((false, false))
        } else {
            Err(error)
        };
    }
    // SAFETY: successful `fstatat` initialized the complete status value.
    let status = unsafe { status.assume_init() };
    if is_regular_file_mode(status.st_mode) {
        return Ok((true, true));
    }
    if replace_target_symlink && (status.st_mode as libc::mode_t & libc::S_IFMT) == libc::S_IFLNK {
        return Ok((true, false));
    }
    {
        Err(Error::new(
            ErrorKind::InvalidInput,
            "rooted atomic destination is not a regular file",
        ))
    }
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
pub(in crate::local) fn create_rooted_staged_file(parent: File, relative_parent: &Path) -> Result<RootedStagedFile> {
    retry_rooted_staging_entry(
        DEFAULT_TEMP_ENTRY_RETRIES,
        || {
            #[cfg(feature = "internal-test-support")]
            if super::test_support::is_enabled("rooted-staging-generate") {
                return Err(Error::other("injected rooted staging filename failure"));
            }
            try_random_file_name(
                "qubit-local-files-",
                Some(ROOTED_ATOMIC_TEMP_PREFIX),
                Some(ROOTED_ATOMIC_TEMP_SUFFIX),
            )
        },
        |name| {
            #[cfg(feature = "internal-test-support")]
            if super::test_support::is_enabled("rooted-staging-collision") {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    "injected rooted staging collision",
                ));
            } else if super::test_support::is_enabled("rooted-staging-open") {
                return Err(Error::other("injected rooted staging open failure"));
            }
            let native_name =
                CString::new(name.as_bytes()).expect("random_file_name_with guarantees generated names without NUL");
            open_file_at(
                &parent,
                &native_name,
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
                0o600,
            )
            .map(|file| (native_name, file))
        },
    )
    .map(|(name, native_name, file)| RootedStagedFile::new(parent, native_name, file, relative_parent.join(name)))
}
