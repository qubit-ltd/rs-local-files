// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Native macOS ACL fixtures for local-filesystem integration tests.

use std::ffi::{
    CString,
    c_char,
    c_int,
    c_void,
};
use std::io::{
    self,
    Error,
    ErrorKind,
};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

/// macOS extended ACL type.
const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;
/// macOS allow-entry tag.
const ACL_EXTENDED_ALLOW: c_int = 1;
/// macOS file-read permission.
const ACL_READ_DATA: c_int = 1 << 1;

/// Opaque native ACL pointer.
type Acl = *mut c_void;
/// Opaque native ACL-entry pointer.
type AclEntry = *mut c_void;
/// Opaque native permission-set pointer.
type AclPermset = *mut c_void;

unsafe extern "C" {
    /// Allocates an ACL capable of holding `count` entries.
    fn acl_init(count: c_int) -> Acl;
    /// Adds an entry to an ACL, potentially updating the ACL pointer.
    fn acl_create_entry(acl: *mut Acl, entry: *mut AclEntry) -> c_int;
    /// Gets the mutable permission set attached to an ACL entry.
    fn acl_get_permset(entry: AclEntry, permset: *mut AclPermset) -> c_int;
    /// Removes all permissions from a permission set.
    fn acl_clear_perms(permset: AclPermset) -> c_int;
    /// Adds one permission to a permission set.
    fn acl_add_perm(permset: AclPermset, permission: c_int) -> c_int;
    /// Attaches a permission set to an ACL entry.
    fn acl_set_permset(entry: AclEntry, permset: AclPermset) -> c_int;
    /// Sets the allow or deny tag on an ACL entry.
    fn acl_set_tag_type(entry: AclEntry, tag: c_int) -> c_int;
    /// Sets the UUID qualifier on an ACL entry.
    fn acl_set_qualifier(entry: AclEntry, qualifier: *const c_void) -> c_int;
    /// Installs an ACL on a filesystem path.
    fn acl_set_file(path: *const c_char, acl_type: c_int, acl: Acl) -> c_int;
    /// Reads an ACL from a filesystem path.
    fn acl_get_file(path: *const c_char, acl_type: c_int) -> Acl;
    /// Formats an ACL into native canonical text.
    fn acl_to_text(acl: Acl, length: *mut isize) -> *mut c_char;
    /// Releases ACL storage or text allocated by the native ACL API.
    fn acl_free(object: *mut c_void) -> c_int;
    /// Maps a user ID to the UUID required by macOS ACL entries.
    fn mbr_uid_to_uuid(uid: libc::uid_t, uuid: *mut u8) -> c_int;
}

/// Installs one explicit read allow-entry for the current user.
pub(crate) fn set_current_user_read_acl(path: &Path) -> io::Result<()> {
    let native_path = native_path(path)?;
    // SAFETY: `acl_init` has no pointer arguments and returns owned storage.
    let mut acl = unsafe { acl_init(1) };
    if acl.is_null() {
        return Err(Error::last_os_error());
    }

    let result = (|| {
        let mut entry = ptr::null_mut();
        // SAFETY: `acl` is live and both output pointers are valid.
        cvt(unsafe { acl_create_entry(&mut acl, &mut entry) })?;

        let mut uuid = [0_u8; 16];
        // SAFETY: `uuid` points to the 16 writable bytes required by uuid_t.
        let status =
            unsafe { mbr_uid_to_uuid(libc::geteuid(), uuid.as_mut_ptr()) };
        if status != 0 {
            return Err(Error::from_raw_os_error(status));
        }

        let mut permset = ptr::null_mut();
        // SAFETY: `entry` belongs to the live ACL and the output pointer is
        // valid.
        cvt(unsafe { acl_get_permset(entry, &mut permset) })?;
        // SAFETY: `permset` belongs to `entry` and remains live with the ACL.
        cvt(unsafe { acl_clear_perms(permset) })?;
        // SAFETY: `ACL_READ_DATA` is a valid macOS ACL permission.
        cvt(unsafe { acl_add_perm(permset, ACL_READ_DATA) })?;
        // SAFETY: `entry` is live and the tag is valid for extended ACLs.
        cvt(unsafe { acl_set_tag_type(entry, ACL_EXTENDED_ALLOW) })?;
        // SAFETY: macOS copies the 16-byte UUID qualifier during this call.
        cvt(unsafe { acl_set_qualifier(entry, uuid.as_ptr().cast()) })?;
        // SAFETY: both arguments belong to the same live ACL entry.
        cvt(unsafe { acl_set_permset(entry, permset) })?;
        // SAFETY: `native_path` is NUL-terminated and `acl` remains live.
        cvt(unsafe {
            acl_set_file(native_path.as_ptr(), ACL_TYPE_EXTENDED, acl)
        })
    })();

    finish_with_acl_cleanup(result, acl)
}

/// Reads the canonical text of a macOS extended ACL.
pub(crate) fn read_macos_acl_text(path: &Path) -> io::Result<Vec<u8>> {
    let native_path = native_path(path)?;
    // SAFETY: `native_path` is NUL-terminated and the ACL type is valid.
    let acl = unsafe { acl_get_file(native_path.as_ptr(), ACL_TYPE_EXTENDED) };
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

/// Converts a path to the native NUL-terminated representation.
fn native_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        Error::new(ErrorKind::InvalidInput, "path contains an interior NUL")
    })
}

/// Converts a native zero/sentinel return convention into `io::Result`.
fn cvt(status: c_int) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

/// Frees one native ACL allocation.
fn free_native(object: *mut c_void) -> io::Result<()> {
    // SAFETY: callers pass storage returned by a native ACL allocation API.
    cvt(unsafe { acl_free(object) })
}

/// Returns the operation result while still checking ACL cleanup.
fn finish_with_acl_cleanup(result: io::Result<()>, acl: Acl) -> io::Result<()> {
    let cleanup = free_native(acl);
    result.and(cleanup)
}
