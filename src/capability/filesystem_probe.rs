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
use super::LocalFileSystemSpace;
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

/// Reads dynamic capacity from an already-opened filesystem handle.
///
/// Probe failures are represented by unavailable dimensions.
#[inline]
pub(crate) fn space(file: &File) -> LocalFileSystemSpace {
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;
        use std::os::fd::AsRawFd;

        let mut status = MaybeUninit::<libc::statvfs>::zeroed();
        // SAFETY: `file` owns a live descriptor and `status` is writable
        // storage for the complete `fstatvfs` result.
        if unsafe { libc::fstatvfs(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
            return LocalFileSystemSpace::new(None, None, None);
        }
        // SAFETY: successful `fstatvfs` initialized the complete value.
        let status = unsafe { status.assume_init() };
        let fragment_size = status.f_frsize as u128;
        let bytes = |blocks| u64::try_from((blocks as u128).checked_mul(fragment_size)?).ok();
        LocalFileSystemSpace::new(bytes(status.f_blocks), bytes(status.f_bfree), bytes(status.f_bavail))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        LocalFileSystemSpace::new(None, None, None)
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
