// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix filesystem limit and space probes.

use std::fs::File;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;

use crate::LocalFileSystemLimits;
use crate::LocalFileSystemSpace;
use crate::SizeLimit;

/// Reads stable path limits from an already-opened filesystem handle.
///
/// Probe failures are represented as unknown limits rather than operation
/// failures.
pub(super) fn limits(file: &File) -> LocalFileSystemLimits {
    LocalFileSystemLimits::new(
        pathconf(file, libc::_PC_PATH_MAX),
        pathconf(file, libc::_PC_NAME_MAX),
    )
}

/// Reads dynamic capacity values from an already-opened filesystem handle.
///
/// Probe failures are represented as unavailable values rather than operation
/// failures.
pub(super) fn space(file: &File) -> LocalFileSystemSpace {
    let mut status = MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `file` owns a live descriptor and `status` is writable storage
    // for the complete `fstatvfs` result.
    if unsafe { libc::fstatvfs(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return LocalFileSystemSpace::new(None, None, None);
    }
    // SAFETY: successful `fstatvfs` initialized the complete status value.
    let status = unsafe { status.assume_init() };
    let fragment_size = status.f_frsize as u128;
    let bytes = |blocks| {
        u64::try_from((blocks as u128).checked_mul(fragment_size)?).ok()
    };
    LocalFileSystemSpace::new(
        bytes(status.f_blocks),
        bytes(status.f_bfree),
        bytes(status.f_bavail),
    )
}

/// Converts one `fpathconf` result into the explicit public limit state.
fn pathconf(file: &File, name: libc::c_int) -> SizeLimit {
    // SAFETY: `file` owns a live descriptor and `fpathconf` does not retain it.
    let value = unsafe { libc::fpathconf(file.as_raw_fd(), name) };
    u64::try_from(value).map_or(SizeLimit::Unknown, SizeLimit::Maximum)
}
