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
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::os::fd::AsRawFd;

/// Copies the complete descriptor-visible xattr set to staging.
pub(super) fn preserve_extended_metadata(
    source: &File,
    staging: &File,
) -> Result<()> {
    let source_names = list_xattrs(source)?;
    #[cfg(coverage)]
    let source_names = {
        let mut source_names = source_names;
        if super::super::coverage_fault::is_enabled(
            "atomic-metadata-security-name",
        ) {
            let _ = source_names.insert(b"security.coverage-missing".to_vec());
        }
        source_names
    };
    #[cfg(coverage)]
    let staging_names = if super::super::coverage_fault::is_enabled(
        "atomic-metadata-staging-list",
    ) {
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
        if get_optional_xattr(staging, name)?.as_deref()
            != Some(source_value.as_slice())
        {
            set_xattr(staging, name, &source_value)?;
        }
    }
    Ok(())
}

/// Lists all extended-attribute names visible through a file descriptor.
fn list_xattrs(file: &File) -> Result<BTreeSet<Vec<u8>>> {
    #[cfg(coverage)]
    let forced_error = if super::super::coverage_fault::is_enabled(
        "atomic-metadata-not-supported",
    ) {
        Some(libc::ENOTSUP)
    } else if super::super::coverage_fault::is_enabled("atomic-metadata-list") {
        Some(libc::EIO)
    } else {
        None
    };
    #[cfg(not(coverage))]
    let forced_error = None;
    loop {
        // SAFETY: the file descriptor is live and null output requests the
        // current list length without retaining pointers.
        let length = match forced_error {
            Some(_) => -1,
            None => unsafe {
                libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0)
            },
        };
        if length == -1 {
            let error = match forced_error {
                Some(code) => Error::from_raw_os_error(code),
                None => Error::last_os_error(),
            };
            if is_not_supported(&error) {
                return Ok(BTreeSet::new());
            }
            return Err(error);
        }
        if length == 0 {
            return Ok(BTreeSet::new());
        }
        let mut buffer = vec![0_u8; length as usize];
        // SAFETY: `buffer` is writable for the requested length and the live
        // descriptor and buffer are not retained by the system call.
        #[cfg(coverage)]
        let forced_read_error = super::super::coverage_fault::is_enabled(
            "atomic-metadata-list-read",
        );
        #[cfg(coverage)]
        let forced_range_error = super::super::coverage_fault::is_enabled(
            "atomic-metadata-list-range",
        );
        #[cfg(coverage)]
        let forced_error = if forced_read_error {
            Some(libc::EIO)
        } else if super::super::coverage_fault::take(
            "atomic-metadata-list-range",
        ) {
            Some(libc::ERANGE)
        } else if forced_range_error {
            Some(libc::EIO)
        } else {
            None
        };
        #[cfg(not(coverage))]
        let forced_error = None;
        let read = match forced_error {
            Some(_) => -1,
            None => unsafe {
                libc::flistxattr(
                    file.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            },
        };
        if read == -1 {
            let error = match forced_error {
                Some(code) => Error::from_raw_os_error(code),
                None => Error::last_os_error(),
            };
            if error.raw_os_error() == Some(libc::ERANGE) {
                continue;
            }
            return Err(error);
        }
        buffer.truncate(read as usize);
        return parse_xattr_names(&buffer);
    }
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
    let name = if super::super::coverage_fault::is_enabled(
        "atomic-metadata-invalid-name",
    ) {
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
fn get_optional_xattr(file: &File, name: &[u8]) -> Result<Option<Vec<u8>>> {
    #[cfg(coverage)]
    if super::super::coverage_fault::is_enabled("atomic-metadata-equal-value") {
        return Ok(Some(b"value".to_vec()));
    }
    get_xattr_inner(file, name)
}

/// Implements descriptor-based xattr lookup.
fn get_xattr_inner(file: &File, name: &[u8]) -> Result<Option<Vec<u8>>> {
    #[cfg(coverage)]
    let forced_error = if super::super::coverage_fault::is_enabled(
        "atomic-metadata-source-missing",
    ) {
        Some(libc::ENODATA)
    } else if super::super::coverage_fault::is_enabled("atomic-metadata-read") {
        Some(libc::EIO)
    } else {
        None
    };
    #[cfg(not(coverage))]
    let forced_error = None;
    let name = native_name(name)?;
    loop {
        // SAFETY: the descriptor and name remain live, and null output asks
        // only for the current value length.
        let length = match forced_error {
            Some(_) => -1,
            None => unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    std::ptr::null_mut(),
                    0,
                )
            },
        };
        if length == -1 {
            let error = match forced_error {
                Some(code) => Error::from_raw_os_error(code),
                None => Error::last_os_error(),
            };
            if is_missing_xattr(&error) {
                return Ok(None);
            }
            return Err(error);
        }
        let mut value = vec![0_u8; length as usize];
        // SAFETY: `value` is writable for its full length and the descriptor
        // and name remain live for this non-retaining system call.
        #[cfg(coverage)]
        let forced_read_error = super::super::coverage_fault::is_enabled(
            "atomic-metadata-value-read",
        );
        #[cfg(not(coverage))]
        let forced_read_error = false;
        let read = if forced_read_error {
            -1
        } else {
            unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            }
        };
        if read == -1 {
            let error = if forced_read_error {
                Error::from_raw_os_error(libc::EIO)
            } else {
                Error::last_os_error()
            };
            if error.raw_os_error() == Some(libc::ERANGE) {
                continue;
            }
            if is_missing_xattr(&error) {
                return Ok(None);
            }
            return Err(error);
        }
        value.truncate(read as usize);
        return Ok(Some(value));
    }
}

/// Sets one descriptor-based extended attribute.
fn set_xattr(file: &File, name: &[u8], value: &[u8]) -> Result<()> {
    #[cfg(coverage)]
    let forced_error =
        super::super::coverage_fault::is_enabled("atomic-metadata-write");
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
    let forced_error =
        super::super::coverage_fault::is_enabled("atomic-metadata-remove");
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
fn is_missing_xattr(error: &Error) -> bool {
    error.raw_os_error() == Some(libc::ENODATA)
}

/// Reports that the filesystem exposes no extended-attribute interface.
#[must_use]
fn is_not_supported(error: &Error) -> bool {
    let code = error.raw_os_error();
    code == Some(libc::ENOTSUP) || code == Some(libc::EOPNOTSUPP)
}
