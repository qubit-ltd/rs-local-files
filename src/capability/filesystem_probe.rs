// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Best-effort probing for already opened native filesystem authorities.

use std::fs::File;

use super::LocalFileSystemLimits;
use super::SizeLimit;

/// Reads stable path limits without turning probe failures into I/O failures.
#[inline]
pub(crate) fn limits(file: &File) -> LocalFileSystemLimits {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let descriptor = file.as_raw_fd();
        LocalFileSystemLimits::new(
            pathconf(descriptor, libc::_PC_PATH_MAX),
            pathconf(descriptor, libc::_PC_NAME_MAX),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        LocalFileSystemLimits::new(SizeLimit::Unknown, SizeLimit::Unknown)
    }
}

/// Converts one `fpathconf` result into the explicit public limit state.
#[cfg(unix)]
#[inline]
fn pathconf(descriptor: std::os::fd::RawFd, name: libc::c_int) -> SizeLimit {
    // POSIX uses -1 both for an error and for an indeterminate limit; either
    // result is intentionally represented as Unknown.
    let value = unsafe { libc::fpathconf(descriptor, name) };
    u64::try_from(value).map_or(SizeLimit::Unknown, SizeLimit::Maximum)
}
