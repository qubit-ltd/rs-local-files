// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Platform-native extended-attribute fixtures.

use std::ffi::CString;
use std::io::{Error, ErrorKind, Result};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Sets one extended attribute on a filesystem path.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn set_user_xattr(path: &Path, name: &str, value: &[u8]) -> Result<()> {
    let path = native_path(path)?;
    let name = native_name(name)?;
    // SAFETY: both C strings and the value buffer remain live for the
    // non-retaining system call, and the supplied lengths match the buffers.
    let result = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Sets one macOS extended attribute on a filesystem path.
#[cfg(target_os = "macos")]
pub(crate) fn set_user_xattr(path: &Path, name: &str, value: &[u8]) -> Result<()> {
    let path = native_path(path)?;
    let name = native_name(name)?;
    // SAFETY: both C strings and the value buffer remain live for the
    // non-retaining system call, and position zero addresses the whole xattr.
    let result = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Sets one FreeBSD user-namespace extended attribute.
#[cfg(target_os = "freebsd")]
pub(crate) fn set_user_xattr(path: &Path, name: &str, value: &[u8]) -> Result<()> {
    let path = native_path(path)?;
    let name = native_name(name)?;
    // SAFETY: both C strings and the value buffer remain live for the
    // non-retaining system call, and the supplied length matches the buffer.
    let result = unsafe {
        libc::extattr_set_file(
            path.as_ptr(),
            libc::EXTATTR_NAMESPACE_USER,
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
        )
    };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    if result as usize != value.len() {
        return Err(Error::new(
            ErrorKind::WriteZero,
            "incomplete FreeBSD extended-attribute fixture write",
        ));
    }
    Ok(())
}

/// Gets one extended attribute from a filesystem path.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn get_user_xattr(path: &Path, name: &str) -> Result<Vec<u8>> {
    let path = native_path(path)?;
    let name = native_name(name)?;
    loop {
        // SAFETY: both C strings remain live and null output requests only the
        // current value length.
        let length =
            unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
        if length == -1 {
            return Err(Error::last_os_error());
        }
        let mut value = vec![0_u8; length as usize];
        // SAFETY: `value` is writable for its full reported length and the C
        // strings remain live for this non-retaining call.
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        if read == -1 {
            let error = Error::last_os_error();
            if error.raw_os_error() == Some(libc::ERANGE) {
                continue;
            }
            return Err(error);
        }
        value.truncate(read as usize);
        return Ok(value);
    }
}

/// Gets one macOS extended attribute from a filesystem path.
#[cfg(target_os = "macos")]
pub(crate) fn get_user_xattr(path: &Path, name: &str) -> Result<Vec<u8>> {
    let path = native_path(path)?;
    let name = native_name(name)?;
    loop {
        // SAFETY: both C strings remain live and null output requests only the
        // current value length at position zero.
        let length =
            unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
        if length == -1 {
            return Err(Error::last_os_error());
        }
        if length == 0 {
            return Ok(Vec::new());
        }
        let mut value = vec![0_u8; length as usize];
        // SAFETY: `value` is writable for its full reported length and both C
        // strings remain live for this non-retaining call.
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        if read == -1 {
            let error = Error::last_os_error();
            if error.raw_os_error() == Some(libc::ERANGE) {
                continue;
            }
            return Err(error);
        }
        value.truncate(read as usize);
        return Ok(value);
    }
}

/// Gets one FreeBSD user-namespace extended attribute.
#[cfg(target_os = "freebsd")]
pub(crate) fn get_user_xattr(path: &Path, name: &str) -> Result<Vec<u8>> {
    let path = native_path(path)?;
    let name = native_name(name)?;
    loop {
        // SAFETY: both C strings remain live and null output requests only the
        // current value length.
        let length = unsafe {
            libc::extattr_get_file(
                path.as_ptr(),
                libc::EXTATTR_NAMESPACE_USER,
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if length == -1 {
            return Err(Error::last_os_error());
        }
        if length == 0 {
            return Ok(Vec::new());
        }
        let mut value = vec![0_u8; length as usize];
        // SAFETY: `value` is writable for its full reported length and both C
        // strings remain live for this non-retaining call.
        let read = unsafe {
            libc::extattr_get_file(
                path.as_ptr(),
                libc::EXTATTR_NAMESPACE_USER,
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        if read == -1 {
            let error = Error::last_os_error();
            if error.raw_os_error() == Some(libc::ERANGE) {
                continue;
            }
            return Err(error);
        }
        value.truncate(read as usize);
        return Ok(value);
    }
}

/// Converts a test path to a NUL-terminated native byte string.
fn native_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "xattr path contains NUL"))
}

/// Converts an attribute name to a NUL-terminated native byte string.
fn native_name(name: &str) -> Result<CString> {
    CString::new(name).map_err(|_| Error::new(ErrorKind::InvalidInput, "xattr name contains NUL"))
}
