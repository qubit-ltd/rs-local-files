// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native Unix directory-entry metadata primitives.
// qubit-style: allow source-test-pair

use std::ffi::CString;
use std::ffi::OsString;
use std::fs::File;
use std::io::Error;
use std::io::Result;
use std::os::fd::AsRawFd;
use std::path::Path;

use super::path_operations::add_path_context;

/// A native child name and its no-follow metadata.
pub(crate) type RootedDirectoryEntry = (OsString, libc::stat);

/// Reads no-follow metadata for one child of an open directory.
///
/// Returns `fstatat` errors with `diagnostic_path` as context.
pub(in crate::local::internal) fn stat_child(
    parent: &File,
    name: &CString,
    diagnostic_path: &Path,
) -> Result<libc::stat> {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the output storage, descriptor, and name remain valid for this
    // non-retaining call.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        return Err(add_path_context(
            Error::last_os_error(),
            "inspect rooted directory entry",
            diagnostic_path,
        ));
    }
    // SAFETY: successful `fstatat` initialized the complete value.
    Ok(unsafe { status.assume_init() })
}
