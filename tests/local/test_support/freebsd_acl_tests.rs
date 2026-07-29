// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Native FreeBSD ACL fixtures for local-filesystem integration tests.

use std::ffi::{
    CString,
    c_char,
    c_int,
    c_uint,
    c_void,
};
use std::io::{
    self,
    Error,
    ErrorKind,
};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// FreeBSD POSIX.1e access ACL type.
const ACL_TYPE_ACCESS: c_uint = 0x0000_0002;
/// FreeBSD NFSv4 ACL type.
const ACL_TYPE_NFS4: c_uint = 0x0000_0004;

/// NFSv4 fixture with an explicit deny entry that cannot collapse to mode bits.
const NFS4_ACL_TEXT: &str = concat!(
    "owner@:full_set:allow\n",
    "group@:read_set:allow\n",
    "everyone@:read_set:allow\n",
    "everyone@:execute:deny",
);
/// POSIX.1e fixture with a named-user entry that cannot collapse to mode bits.
const POSIX_ACL_TEXT: &str = concat!(
    "user::rw-\n",
    "group::r--\n",
    "other::---\n",
    "user:1:r--\n",
    "mask::r--",
);

/// Opaque native ACL pointer.
type Acl = *mut c_void;

unsafe extern "C" {
    /// Parses POSIX.1e or NFSv4 ACL text into native storage.
    fn acl_from_text(text: *const c_char) -> Acl;
    /// Installs an ACL on a filesystem path.
    fn acl_set_file(path: *const c_char, acl_type: c_uint, acl: Acl) -> c_int;
    /// Reads an ACL from a filesystem path.
    fn acl_get_file(path: *const c_char, acl_type: c_uint) -> Acl;
    /// Formats an ACL into native canonical text.
    fn acl_to_text(acl: Acl, length: *mut isize) -> *mut c_char;
    /// Releases ACL storage or text allocated by the native ACL API.
    fn acl_free(object: *mut c_void) -> c_int;
}

/// Installs a non-trivial ACL in the flavor supported by the filesystem.
///
/// Returns the installed native ACL type, or `None` when the filesystem has
/// neither NFSv4 nor POSIX.1e ACL support.
pub(crate) fn install_supported_test_acl(
    path: &Path,
) -> io::Result<Option<c_uint>> {
    let native_path = native_path(path)?;
    match install_acl(&native_path, ACL_TYPE_NFS4, NFS4_ACL_TEXT) {
        Ok(()) => return Ok(Some(ACL_TYPE_NFS4)),
        Err(error) if is_unsupported_acl_error(&error) => {}
        Err(error) => return Err(error),
    }
    match install_acl(&native_path, ACL_TYPE_ACCESS, POSIX_ACL_TEXT) {
        Ok(()) => Ok(Some(ACL_TYPE_ACCESS)),
        Err(error) if is_unsupported_acl_error(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Reads the canonical text for one FreeBSD ACL flavor.
pub(crate) fn read_freebsd_acl_text(
    path: &Path,
    acl_type: c_uint,
) -> io::Result<Vec<u8>> {
    let native_path = native_path(path)?;
    // SAFETY: `native_path` is NUL-terminated and `acl_type` was returned by
    // `install_supported_test_acl`.
    let acl = unsafe { acl_get_file(native_path.as_ptr(), acl_type) };
    if acl.is_null() {
        return Err(Error::last_os_error());
    }

    let mut length = 0_isize;
    // SAFETY: `acl` is live and `length` is a valid output pointer.
    let text = unsafe { acl_to_text(acl, &mut length) };
    if text.is_null() {
        let error = Error::last_os_error();
        let _ = free_native(acl);
        return Err(error);
    }

    let result = usize::try_from(length)
        .map_err(|_| {
            Error::new(ErrorKind::InvalidData, "negative ACL text length")
        })
        .map(|length| {
            // SAFETY: `acl_to_text` returned `length` readable bytes.
            unsafe { std::slice::from_raw_parts(text.cast(), length) }.to_vec()
        });
    let text_cleanup = free_native(text.cast());
    let acl_cleanup = free_native(acl);
    let cleanup = text_cleanup.and(acl_cleanup);
    result.and_then(|value| cleanup.map(|()| value))
}

/// Parses and installs one native ACL fixture.
fn install_acl(
    native_path: &CString,
    acl_type: c_uint,
    acl_text: &str,
) -> io::Result<()> {
    let native_text = CString::new(acl_text).expect("ACL fixture has no NUL");
    // SAFETY: `native_text` is NUL-terminated and remains live for the call.
    let acl = unsafe { acl_from_text(native_text.as_ptr()) };
    if acl.is_null() {
        return Err(Error::last_os_error());
    }

    // SAFETY: both the native path and parsed ACL remain live for the call.
    let status = unsafe { acl_set_file(native_path.as_ptr(), acl_type, acl) };
    let result = if status == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    };
    let cleanup = free_native(acl);
    result.and(cleanup)
}

/// Converts a path to the native NUL-terminated representation.
fn native_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        Error::new(ErrorKind::InvalidInput, "path contains an interior NUL")
    })
}

/// Frees one native ACL allocation.
fn free_native(object: *mut c_void) -> io::Result<()> {
    // SAFETY: callers pass storage returned by a native ACL allocation API.
    let status = unsafe { acl_free(object) };
    if status == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Reports that the filesystem does not support a requested ACL flavor.
fn is_unsupported_acl_error(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(code) if
        code == libc::EINVAL
            || code == libc::EOPNOTSUPP
            || code == libc::ENOSYS)
}
