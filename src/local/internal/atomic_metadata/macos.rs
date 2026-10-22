// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! macOS atomic ACL and extended-attribute preservation.
// Private behavior is covered through public integration tests.

use std::fs::File;
use std::io::Error;
use std::io::Result;
use std::os::fd::AsRawFd;

/// Copies the source file's ACLs and extended attributes to the staging file
/// through macOS's native `copyfile` API.
///
/// # Parameters
/// - `source`: Open file whose ACLs and extended attributes are preserved.
/// - `staging`: Open file that receives the source metadata.
///
/// # Errors
/// Returns the operating system error if the native copy operation fails.
pub(super) fn preserve_extended_metadata(source: &File, staging: &File) -> Result<()> {
    let flags = libc::COPYFILE_ACL | libc::COPYFILE_XATTR;
    // SAFETY: both descriptors remain live for this non-retaining call. A
    // null copyfile state requests the default state, and both flags are
    // native metadata-only copyfile flags.
    let result = unsafe { libc::fcopyfile(source.as_raw_fd(), staging.as_raw_fd(), std::ptr::null_mut(), flags) };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}
