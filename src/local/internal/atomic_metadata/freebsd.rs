// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! FreeBSD atomic ACL and extended-attribute preservation.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::collections::BTreeSet;
use std::ffi::{
    CString,
    c_void,
};
use std::fs::File;
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::os::fd::AsRawFd;

/// FreeBSD POSIX.1e access ACL type.
const ACL_TYPE_ACCESS: u32 = 0x0000_0002;
/// FreeBSD NFSv4 ACL type.
const ACL_TYPE_NFS4: u32 = 0x0000_0004;

unsafe extern "C" {
    /// Gets a descriptor ACL of the requested native type.
    fn acl_get_fd_np(file: libc::c_int, acl_type: u32) -> *mut c_void;

    /// Applies a descriptor ACL of the requested native type.
    fn acl_set_fd_np(
        file: libc::c_int,
        acl: *mut c_void,
        acl_type: u32,
    ) -> libc::c_int;

    /// Releases an ACL allocated by the native ACL API.
    fn acl_free(object: *mut c_void) -> libc::c_int;
}

/// Copies the native ACL and both extended-attribute namespaces to staging.
pub(super) fn preserve_extended_metadata(
    source: &File,
    staging: &File,
) -> Result<()> {
    preserve_acl(source, staging)?;
    preserve_namespace(source, staging, libc::EXTATTR_NAMESPACE_USER)?;
    preserve_namespace(source, staging, libc::EXTATTR_NAMESPACE_SYSTEM)
}

/// Copies the source filesystem's supported ACL flavor to staging.
fn preserve_acl(source: &File, staging: &File) -> Result<()> {
    match get_acl(source, ACL_TYPE_NFS4) {
        Ok(acl) => apply_acl(staging, acl, ACL_TYPE_NFS4),
        Err(error) if is_unsupported_acl_type(&error) => {
            let acl = get_acl(source, ACL_TYPE_ACCESS)?;
            apply_acl(staging, acl, ACL_TYPE_ACCESS)
        }
        Err(error) => Err(error),
    }
}

/// Gets an ACL allocated by FreeBSD libc.
fn get_acl(file: &File, acl_type: u32) -> Result<*mut c_void> {
    // SAFETY: the descriptor remains live, and `acl_type` is one of the two
    // native ACL type constants accepted for regular files.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), acl_type) };
    if acl.is_null() {
        return Err(Error::last_os_error());
    }
    Ok(acl)
}

/// Applies and releases one ACL, retaining the application error if any.
fn apply_acl(staging: &File, acl: *mut c_void, acl_type: u32) -> Result<()> {
    // SAFETY: `acl` came from `acl_get_fd_np`, the staging descriptor remains
    // live, and `acl_type` is the same native flavor used to obtain the ACL.
    let set_result =
        unsafe { acl_set_fd_np(staging.as_raw_fd(), acl, acl_type) };
    let set_error = (set_result == -1).then(Error::last_os_error);
    // SAFETY: `acl` is a non-null allocation returned by `acl_get_fd_np` and
    // has not previously been released.
    let free_result = unsafe { acl_free(acl) };
    if let Some(error) = set_error {
        return Err(error);
    }
    if free_result == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Synchronizes one FreeBSD extended-attribute namespace.
fn preserve_namespace(
    source: &File,
    staging: &File,
    namespace: libc::c_int,
) -> Result<()> {
    let source_names = list_attributes(source, namespace)?;
    let staging_names = list_attributes(staging, namespace)?;
    for name in staging_names.difference(&source_names) {
        remove_attribute(staging, namespace, name)?;
    }
    for name in &source_names {
        let source_value = get_attribute(source, namespace, name)?;
        if get_optional_attribute(staging, namespace, name)?.as_deref()
            != Some(source_value.as_slice())
        {
            set_attribute(staging, namespace, name, &source_value)?;
        }
    }
    Ok(())
}

/// Lists the length-prefixed names in one descriptor namespace.
fn list_attributes(
    file: &File,
    namespace: libc::c_int,
) -> Result<BTreeSet<Vec<u8>>> {
    loop {
        // SAFETY: the descriptor remains live and null output requests only
        // the current byte length of the namespace's name list.
        let length = unsafe {
            libc::extattr_list_fd(
                file.as_raw_fd(),
                namespace,
                std::ptr::null_mut(),
                0,
            )
        };
        if length == -1 {
            let error = Error::last_os_error();
            if is_not_supported(&error) {
                return Ok(BTreeSet::new());
            }
            return Err(error);
        }
        if length == 0 {
            return Ok(BTreeSet::new());
        }
        let mut buffer = vec![0_u8; length as usize];
        // SAFETY: `buffer` is writable for its full length and the descriptor
        // remains live for this non-retaining call.
        let read = unsafe {
            libc::extattr_list_fd(
                file.as_raw_fd(),
                namespace,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if read == -1 {
            let error = Error::last_os_error();
            if error.raw_os_error() == Some(libc::ERANGE) {
                continue;
            }
            return Err(error);
        }
        buffer.truncate(read as usize);
        return parse_attribute_names(&buffer);
    }
}

/// Parses FreeBSD's one-byte-length-prefixed attribute-name list.
fn parse_attribute_names(buffer: &[u8]) -> Result<BTreeSet<Vec<u8>>> {
    let mut names = BTreeSet::new();
    let mut offset = 0;
    while offset < buffer.len() {
        let length = usize::from(buffer[offset]);
        offset += 1;
        let end = offset.checked_add(length).ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidData,
                "extended-attribute name length overflow",
            )
        })?;
        if length == 0 || end > buffer.len() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "invalid FreeBSD extended-attribute name list",
            ));
        }
        let name = &buffer[offset..end];
        if name.contains(&0) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "extended-attribute name contains NUL",
            ));
        }
        let _ = names.insert(name.to_vec());
        offset = end;
    }
    Ok(names)
}

/// Gets one required extended-attribute value.
fn get_attribute(
    file: &File,
    namespace: libc::c_int,
    name: &[u8],
) -> Result<Vec<u8>> {
    get_optional_attribute(file, namespace, name)?.ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            "source extended attribute disappeared during preservation",
        )
    })
}

/// Gets one optional extended-attribute value, retrying size races.
fn get_optional_attribute(
    file: &File,
    namespace: libc::c_int,
    name: &[u8],
) -> Result<Option<Vec<u8>>> {
    let name = native_name(name)?;
    loop {
        // SAFETY: the descriptor and name remain live, and null output asks
        // only for the current value length.
        let length = unsafe {
            libc::extattr_get_fd(
                file.as_raw_fd(),
                namespace,
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if length == -1 {
            let error = Error::last_os_error();
            if is_missing_attribute(&error) {
                return Ok(None);
            }
            return Err(error);
        }
        if length == 0 {
            return Ok(Some(Vec::new()));
        }
        let mut value = vec![0_u8; length as usize];
        // SAFETY: `value` is writable for its full length and the descriptor
        // and name remain live for this non-retaining call.
        let read = unsafe {
            libc::extattr_get_fd(
                file.as_raw_fd(),
                namespace,
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
            if is_missing_attribute(&error) {
                return Ok(None);
            }
            return Err(error);
        }
        value.truncate(read as usize);
        return Ok(Some(value));
    }
}

/// Sets one descriptor-based extended attribute.
fn set_attribute(
    file: &File,
    namespace: libc::c_int,
    name: &[u8],
    value: &[u8],
) -> Result<()> {
    let name = native_name(name)?;
    // SAFETY: the descriptor, name, and value remain live for the call, and
    // the supplied length matches the value buffer.
    let result = unsafe {
        libc::extattr_set_fd(
            file.as_raw_fd(),
            namespace,
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
            "incomplete FreeBSD extended-attribute write",
        ));
    }
    Ok(())
}

/// Removes one descriptor-based extended attribute.
fn remove_attribute(
    file: &File,
    namespace: libc::c_int,
    name: &[u8],
) -> Result<()> {
    let name = native_name(name)?;
    // SAFETY: the descriptor and name remain live for this non-retaining call.
    let result = unsafe {
        libc::extattr_delete_fd(file.as_raw_fd(), namespace, name.as_ptr())
    };
    if result == -1 {
        let error = Error::last_os_error();
        if is_missing_attribute(&error) {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

/// Converts an attribute name to a native C string.
fn native_name(name: &[u8]) -> Result<CString> {
    CString::new(name).map_err(|_| {
        Error::new(
            ErrorKind::InvalidData,
            "extended-attribute name contains NUL",
        )
    })
}

/// Reports an absent FreeBSD extended attribute.
fn is_missing_attribute(error: &Error) -> bool {
    error.raw_os_error() == Some(libc::ENOATTR)
}

/// Reports that a filesystem lacks an extended-attribute namespace.
fn is_not_supported(error: &Error) -> bool {
    let code = error.raw_os_error();
    code == Some(libc::ENOTSUP) || code == Some(libc::EOPNOTSUPP)
}

/// Reports that a filesystem does not use the requested ACL flavor.
fn is_unsupported_acl_type(error: &Error) -> bool {
    let code = error.raw_os_error();
    code == Some(libc::EINVAL)
        || code == Some(libc::ENOTSUP)
        || code == Some(libc::EOPNOTSUPP)
}
