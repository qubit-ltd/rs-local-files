// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Linux and Android atomic extended-metadata preservation.
// qubit-style: allow source-test-pair
// qubit-style: allow coverage-cfg
// Private behavior is covered through public integration tests.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::os::fd::AsRawFd;

/// Maximum number of attempts for one xattr size race.
const XATTR_SIZE_RACE_ATTEMPTS: usize = 8;

/// Copies the complete descriptor-visible xattr set to staging.
pub(super) fn preserve_extended_metadata(source: &File, staging: &File) -> Result<()> {
    let source_names = list_xattrs(source)?;
    #[cfg(coverage)]
    let source_names = {
        let mut source_names = source_names;
        if super::super::coverage_fault::is_enabled("atomic-metadata-security-name") {
            let _ = source_names.insert(b"security.coverage-missing".to_vec());
        }
        source_names
    };
    #[cfg(coverage)]
    let staging_names = if super::super::coverage_fault::is_enabled("atomic-metadata-staging-list")
    {
        Err(Error::from_raw_os_error(libc::EIO))
    } else {
        list_xattrs(staging)
    }?;
    #[cfg(not(coverage))]
    let staging_names = list_xattrs(staging)?;
    #[cfg(coverage)]
    let staging_names = {
        let mut staging_names = staging_names;
        if super::super::coverage_fault::is_enabled("atomic-metadata-remove") {
            let _ = staging_names.insert(b"user.coverage-remove".to_vec());
        }
        staging_names
    };
    for name in staging_names.difference(&source_names) {
        remove_xattr(staging, name)?;
    }
    for name in ordered_names(&source_names) {
        let source_value = get_xattr(source, name)?;
        if get_optional_xattr(staging, name)?.as_deref() != Some(source_value.as_slice()) {
            set_xattr(staging, name, &source_value)?;
        }
    }
    Ok(())
}

/// Lists all extended-attribute names visible through a file descriptor.
fn list_xattrs(file: &File) -> Result<BTreeSet<Vec<u8>>> {
    let mut remaining_attempts = XATTR_SIZE_RACE_ATTEMPTS;
    loop {
        let length = match query_xattr_list_length(file) {
            Ok(0) => return Ok(BTreeSet::new()),
            Ok(length) => length,
            Err(error) if is_not_supported(&error) => {
                return Ok(BTreeSet::new());
            }
            Err(error) => return Err(error),
        };
        let mut buffer = vec![0_u8; length];
        match read_xattr_list(file, &mut buffer) {
            Ok(read) => {
                buffer.truncate(read);
                return parse_xattr_names(&buffer);
            }
            Err(error) if error.raw_os_error() == Some(libc::ERANGE) && remaining_attempts > 1 => {
                remaining_attempts -= 1;
                continue;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Queries the buffer length required for descriptor-based xattr names.
///
/// # Parameters
/// - `file`: Open file whose extended attributes should be listed.
///
/// # Returns
/// Required buffer length in bytes, including zero for an empty list.
///
/// # Errors
/// Returns the native `flistxattr` error or a selected coverage fault.
fn query_xattr_list_length(file: &File) -> Result<usize> {
    #[cfg(coverage)]
    let forced_error = if super::super::coverage_fault::is_enabled("atomic-metadata-not-supported")
    {
        Some(libc::ENOTSUP)
    } else if super::super::coverage_fault::is_enabled("atomic-metadata-list") {
        Some(libc::EIO)
    } else {
        None
    };
    #[cfg(not(coverage))]
    let forced_error = None;
    // SAFETY: the file descriptor is live and null output requests the current
    // list length without retaining pointers.
    let length = match forced_error {
        Some(_) => -1,
        None => unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) },
    };
    xattr_size_result(length, forced_error)
}

/// Reads descriptor-based xattr names into a caller-provided buffer.
///
/// # Parameters
/// - `file`: Open file whose extended attributes should be listed.
/// - `buffer`: Writable storage sized by [`query_xattr_list_length`].
///
/// # Returns
/// Number of initialized bytes in `buffer`.
///
/// # Errors
/// Returns the native `flistxattr` error or a selected coverage fault.
fn read_xattr_list(file: &File, buffer: &mut [u8]) -> Result<usize> {
    #[cfg(coverage)]
    let forced_error = if super::super::coverage_fault::is_enabled("atomic-metadata-list-read") {
        Some(libc::EIO)
    } else if super::super::coverage_fault::is_enabled("atomic-metadata-list-range-persistent")
        || super::super::coverage_fault::take("atomic-metadata-list-range")
    {
        Some(libc::ERANGE)
    } else if super::super::coverage_fault::is_enabled("atomic-metadata-list-range") {
        Some(libc::EIO)
    } else {
        None
    };
    #[cfg(not(coverage))]
    let forced_error = None;
    // SAFETY: `buffer` is writable for the requested length and the live
    // descriptor and buffer are not retained by the system call.
    let read = match forced_error {
        Some(_) => -1,
        None => unsafe {
            libc::flistxattr(file.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len())
        },
    };
    xattr_size_result(read, forced_error)
}

/// Parses the NUL-separated name list returned by `flistxattr`.
fn parse_xattr_names(buffer: &[u8]) -> Result<BTreeSet<Vec<u8>>> {
    let mut names = BTreeSet::new();
    for name in buffer.split(|byte| *byte == 0) {
        if name.is_empty() {
            continue;
        }
        let _ = names.insert(name.to_vec());
    }
    Ok(names)
}

/// Returns names in deterministic order with security attributes last.
#[must_use]
fn ordered_names(names: &BTreeSet<Vec<u8>>) -> Vec<&[u8]> {
    let mut ordinary = Vec::new();
    let mut security = Vec::new();
    for name in names {
        if name.starts_with(b"security.") {
            security.push(name.as_slice());
        } else {
            ordinary.push(name.as_slice());
        }
    }
    ordinary.extend(security);
    ordinary
}

/// Gets one required extended-attribute value, retrying size races.
fn get_xattr(file: &File, name: &[u8]) -> Result<Vec<u8>> {
    #[cfg(coverage)]
    let name = if super::super::coverage_fault::is_enabled("atomic-metadata-invalid-name") {
        b"user.coverage\0invalid".as_slice()
    } else {
        name
    };
    match get_xattr_inner(file, name)? {
        Some(value) => Ok(value),
        None => Err(Error::new(
            ErrorKind::NotFound,
            "source extended attribute disappeared during preservation",
        )),
    }
}

/// Gets an optional extended-attribute value, retrying size races.
#[inline]
fn get_optional_xattr(file: &File, name: &[u8]) -> Result<Option<Vec<u8>>> {
    #[cfg(coverage)]
    if super::super::coverage_fault::is_enabled("atomic-metadata-equal-value") {
        return Ok(Some(b"value".to_vec()));
    }
    get_xattr_inner(file, name)
}

/// Implements descriptor-based xattr lookup.
fn get_xattr_inner(file: &File, name: &[u8]) -> Result<Option<Vec<u8>>> {
    let name = native_name(name)?;
    let mut remaining_attempts = XATTR_SIZE_RACE_ATTEMPTS;
    loop {
        let length = match query_xattr_value_length(file, &name) {
            Ok(length) => length,
            Err(error) if is_missing_xattr(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut value = vec![0_u8; length];
        match read_xattr_value(file, &name, &mut value) {
            Ok(read) => {
                value.truncate(read);
                return Ok(Some(value));
            }
            Err(error) if error.raw_os_error() == Some(libc::ERANGE) && remaining_attempts > 1 => {
                remaining_attempts -= 1;
                continue;
            }
            Err(error) if is_missing_xattr(&error) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
}

/// Queries the buffer length required for one descriptor-based xattr value.
///
/// # Parameters
/// - `file`: Open file containing the attribute.
/// - `name`: Native attribute name.
///
/// # Returns
/// Required value-buffer length in bytes.
///
/// # Errors
/// Returns the native `fgetxattr` error or a selected coverage fault.
fn query_xattr_value_length(file: &File, name: &CString) -> Result<usize> {
    #[cfg(coverage)]
    let forced_error = if super::super::coverage_fault::is_enabled("atomic-metadata-source-missing")
    {
        Some(libc::ENODATA)
    } else if super::super::coverage_fault::is_enabled("atomic-metadata-read") {
        Some(libc::EIO)
    } else {
        None
    };
    #[cfg(not(coverage))]
    let forced_error = None;
    // SAFETY: the descriptor and name remain live, and null output asks only
    // for the current value length.
    let length = match forced_error {
        Some(_) => -1,
        None => unsafe {
            libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0)
        },
    };
    xattr_size_result(length, forced_error)
}

/// Reads one descriptor-based xattr value into a caller-provided buffer.
///
/// # Parameters
/// - `file`: Open file containing the attribute.
/// - `name`: Native attribute name.
/// - `value`: Writable storage sized by [`query_xattr_value_length`].
///
/// # Returns
/// Number of initialized bytes in `value`.
///
/// # Errors
/// Returns the native `fgetxattr` error or a selected coverage fault.
fn read_xattr_value(file: &File, name: &CString, value: &mut [u8]) -> Result<usize> {
    #[cfg(coverage)]
    let forced_error =
        if super::super::coverage_fault::is_enabled("atomic-metadata-value-range-persistent") {
            Some(libc::ERANGE)
        } else if super::super::coverage_fault::is_enabled("atomic-metadata-value-read") {
            Some(libc::EIO)
        } else {
            None
        };
    #[cfg(not(coverage))]
    let forced_error = None;
    // SAFETY: `value` is writable for its full length and the descriptor and
    // name remain live for this non-retaining system call.
    let read = match forced_error {
        Some(_) => -1,
        None => unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        },
    };
    xattr_size_result(read, forced_error)
}

/// Converts one size-returning xattr syscall result to an I/O result.
///
/// # Parameters
/// - `result`: Native byte count or `-1` on failure.
/// - `forced_error`: Coverage-only native error code selected for this call.
///
/// # Returns
/// Nonnegative byte count returned by the native syscall.
///
/// # Errors
/// Returns the injected error or the operating system's last error when the
/// native syscall returns `-1`.
#[inline]
fn xattr_size_result(result: isize, forced_error: Option<i32>) -> Result<usize> {
    if result == -1 {
        Err(forced_error.map_or_else(Error::last_os_error, Error::from_raw_os_error))
    } else {
        Ok(result as usize)
    }
}

/// Sets one descriptor-based extended attribute.
fn set_xattr(file: &File, name: &[u8], value: &[u8]) -> Result<()> {
    #[cfg(coverage)]
    let forced_error = super::super::coverage_fault::is_enabled("atomic-metadata-write");
    #[cfg(not(coverage))]
    let forced_error = false;
    let name = native_name(name)?;
    // SAFETY: the descriptor, name, and value remain live for the call, and
    // the supplied length matches the value buffer.
    let result = if forced_error {
        -1
    } else {
        unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        }
    };
    if result == -1 {
        return Err(if forced_error {
            Error::from_raw_os_error(libc::EIO)
        } else {
            Error::last_os_error()
        });
    }
    Ok(())
}

/// Removes one descriptor-based extended attribute.
fn remove_xattr(file: &File, name: &[u8]) -> Result<()> {
    #[cfg(coverage)]
    let forced_error = super::super::coverage_fault::is_enabled("atomic-metadata-remove");
    #[cfg(not(coverage))]
    let forced_error = false;
    let name = native_name(name)?;
    // SAFETY: the descriptor and name remain live for this non-retaining call.
    let result = if forced_error {
        -1
    } else {
        unsafe { libc::fremovexattr(file.as_raw_fd(), name.as_ptr()) }
    };
    if result == -1 {
        let error = if forced_error {
            Error::from_raw_os_error(libc::EIO)
        } else {
            Error::last_os_error()
        };
        if is_missing_xattr(&error) {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

/// Converts an xattr name to a native C string.
#[inline]
fn native_name(name: &[u8]) -> Result<CString> {
    match CString::new(name) {
        Ok(name) => Ok(name),
        Err(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "extended-attribute name contains NUL",
        )),
    }
}

/// Reports the platform's missing-attribute error.
#[must_use]
#[inline]
fn is_missing_xattr(error: &Error) -> bool {
    error.raw_os_error() == Some(libc::ENODATA)
}

/// Reports that the filesystem exposes no extended-attribute interface.
#[must_use]
#[inline]
fn is_not_supported(error: &Error) -> bool {
    let code = error.raw_os_error();
    code == Some(libc::ENOTSUP) || code == Some(libc::EOPNOTSUPP)
}
