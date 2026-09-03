// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Best-effort probing for already opened native filesystem authorities.

use std::fs::File;
use std::io;

use super::LocalFileSystemLimits;
use super::LocalFileSystemSpace;
#[cfg(not(windows))]
use super::LocalPathLengthUnit;
#[cfg(not(windows))]
use super::SizeLimit;

/// Reads stable path limits from an already-opened filesystem authority.
///
/// # Errors
///
/// Returns the native query error when an objective limit cannot be observed.
///
/// # Parameters
///
/// - `file`: Open file handle queried for filesystem information.
///
/// # Returns
///
/// The filesystem limits observed from the open authority.
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn limits(file: &File) -> io::Result<LocalFileSystemLimits> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let descriptor = file.as_raw_fd();
        Ok(LocalFileSystemLimits::new(
            pathconf(descriptor, libc::_PC_PATH_MAX)?,
            pathconf(descriptor, libc::_PC_NAME_MAX)?,
            LocalPathLengthUnit::Bytes,
        ))
    }
    #[cfg(windows)]
    {
        crate::local::probe_windows_limits(file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Ok(LocalFileSystemLimits::new(
            SizeLimit::Unknown,
            SizeLimit::Unknown,
            LocalPathLengthUnit::Bytes,
        ))
    }
}

/// Reads dynamic capacity from an already-opened filesystem handle.
///
/// # Errors
///
/// Returns the native query error when capacity cannot be observed.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn space(file: &File) -> io::Result<LocalFileSystemSpace> {
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;
        use std::os::fd::AsRawFd;

        let mut status = MaybeUninit::<libc::statvfs>::zeroed();
        // SAFETY: `file` owns a live descriptor and `status` is writable
        // storage for the complete `fstatvfs` result.
        if unsafe { libc::fstatvfs(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful `fstatvfs` initialized the complete value.
        let status = unsafe { status.assume_init() };
        let fragment_size = status.f_frsize as u128;
        let bytes = |blocks| u64::try_from((blocks as u128).checked_mul(fragment_size)?).ok();
        Ok(LocalFileSystemSpace::new(
            bytes(status.f_blocks),
            bytes(status.f_bfree),
            bytes(status.f_bavail),
        ))
    }
    #[cfg(windows)]
    {
        crate::local::probe_windows_space(file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Ok(LocalFileSystemSpace::new(None, None, None))
    }
}

/// Converts one `fpathconf` result into the explicit public limit state.
#[cfg(unix)]
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn pathconf(descriptor: std::os::fd::RawFd, name: libc::c_int) -> io::Result<SizeLimit> {
    let errno_available = clear_errno();
    let value = unsafe { libc::fpathconf(descriptor, name) };
    if value >= 0 {
        return Ok(SizeLimit::Maximum(value as u64));
    }
    if !errno_available {
        return Ok(SizeLimit::Unknown);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(0) {
        Ok(SizeLimit::Unknown)
    } else {
        Err(error)
    }
}

/// Clears the calling thread's POSIX errno before an indeterminate query.
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "emscripten",
    target_os = "fuchsia",
    target_os = "hurd",
    target_os = "redox"
))]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // SAFETY: `__errno_location` returns this thread's writable errno slot.
    unsafe { *libc::__errno_location() = 0 };
    true
}

/// Clears the calling thread's POSIX errno before an indeterminate query.
#[cfg(target_os = "android")]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // SAFETY: `__errno` returns this thread's writable errno slot.
    unsafe { *libc::__errno() = 0 };
    true
}

/// Clears the calling thread's POSIX errno before an indeterminate query.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // SAFETY: `__error` returns this thread's writable errno slot.
    unsafe { *libc::__error() = 0 };
    true
}

/// Clears the calling thread's POSIX errno before an indeterminate query.
#[cfg(any(target_os = "netbsd", target_os = "openbsd"))]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // SAFETY: `__errno` returns this thread's writable errno slot.
    unsafe { *libc::__errno() = 0 };
    true
}

/// Clears the calling thread's POSIX errno before an indeterminate query.
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // SAFETY: `___errno` returns this thread's writable errno slot.
    unsafe { *libc::___errno() = 0 };
    true
}

/// Clears the calling thread's POSIX errno before an indeterminate query.
#[cfg(target_os = "aix")]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // SAFETY: `_Errno` returns this thread's writable errno slot.
    unsafe { *libc::_Errno() = 0 };
    true
}

/// Clears the calling thread's POSIX errno before an indeterminate query.
#[cfg(target_os = "haiku")]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // SAFETY: `_errnop` returns this thread's writable errno slot.
    unsafe { *libc::_errnop() = 0 };
    true
}

/// Leaves errno untouched on Unix targets whose libc does not expose one of
/// the supported thread-local accessors.
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "dragonfly",
        target_os = "emscripten",
        target_os = "fuchsia",
        target_os = "hurd",
        target_os = "redox",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "aix",
        target_os = "haiku"
    ))
))]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn clear_errno() -> bool {
    // Without a portable setter, a -1 result remains indeterminate rather
    // than being misclassified from stale thread-local errno.
    false
}
