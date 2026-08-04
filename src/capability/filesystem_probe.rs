// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Best-effort probing for already opened native filesystem authorities.

use std::fs::File;

use super::{
    LocalFileSystemLimits,
    LocalFileSystemSpace,
    SizeLimit,
};

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

/// Reads dynamic space values without turning probe failures into I/O failures.
#[inline]
pub(crate) fn space(file: &File) -> LocalFileSystemSpace {
    #[cfg(unix)]
    {
        use std::{
            mem::MaybeUninit,
            os::fd::AsRawFd,
        };
        let mut stat = MaybeUninit::<libc::statvfs>::zeroed();
        // SAFETY: `file` owns a live descriptor and `stat` is writable.
        if unsafe { libc::fstatvfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return LocalFileSystemSpace::new(None, None, None);
        }
        let stat = unsafe { stat.assume_init() };
        let block_size = u64::from(stat.f_frsize);
        LocalFileSystemSpace::new(
            u64::from(stat.f_blocks).checked_mul(block_size),
            u64::from(stat.f_bfree).checked_mul(block_size),
            u64::from(stat.f_bavail).checked_mul(block_size),
        )
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
