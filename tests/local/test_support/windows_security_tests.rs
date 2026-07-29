// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Native Windows security and alternate-stream fixtures.

use std::ffi::OsString;
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, CreateWellKnownSid,
    DACL_SECURITY_INFORMATION, GetFileSecurityW, InitializeAcl, InitializeSecurityDescriptor,
    SECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE, SetFileSecurityW, SetSecurityDescriptorDacl,
    WinWorldSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_ATTRIBUTE_READONLY, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
    SetFileAttributesW,
};

/// Windows security-descriptor revision accepted by the initialization API.
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

/// Installs a deterministic full-control DACL for the world SID.
pub(crate) fn set_world_full_control_dacl(path: &Path) -> Result<()> {
    let mut sid_size = SECURITY_MAX_SID_SIZE;
    let mut sid_storage = aligned_storage(sid_size)?;
    let sid = sid_storage.as_mut_ptr().cast();
    // SAFETY: `sid_storage` is aligned and writable for `sid_size` bytes, the
    // size pointer remains live, and a null domain SID is valid for this well-
    // known SID type.
    let created =
        unsafe { CreateWellKnownSid(WinWorldSid, std::ptr::null_mut(), sid, &raw mut sid_size) };
    if created == 0 {
        return Err(Error::last_os_error());
    }

    let acl_size = size_of::<ACL>()
        .checked_add(size_of::<ACCESS_ALLOWED_ACE>())
        .and_then(|size| size.checked_sub(size_of::<u32>()))
        .and_then(|size| size.checked_add(sid_size as usize))
        .ok_or_else(|| Error::other("Windows ACL fixture size overflow"))?;
    let acl_size = u32::try_from(acl_size)
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "Windows ACL fixture is too large"))?;
    let mut acl_storage = aligned_storage(acl_size)?;
    let acl = acl_storage.as_mut_ptr().cast::<ACL>();
    // SAFETY: `acl_storage` is aligned and writable for `acl_size` bytes, and
    // ACL_REVISION is the revision required by AddAccessAllowedAce.
    let initialized = unsafe { InitializeAcl(acl, acl_size, ACL_REVISION) };
    if initialized == 0 {
        return Err(Error::last_os_error());
    }
    // SAFETY: `acl` is initialized with sufficient storage, `sid` is a live
    // well-known SID, and FILE_ALL_ACCESS is a valid file access mask.
    let added = unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid) };
    if added == 0 {
        return Err(Error::last_os_error());
    }

    let mut descriptor = SECURITY_DESCRIPTOR::default();
    // SAFETY: `descriptor` is writable and remains live through the calls that
    // initialize it, attach the live ACL, and synchronously apply it.
    let initialized = unsafe {
        InitializeSecurityDescriptor((&raw mut descriptor).cast(), SECURITY_DESCRIPTOR_REVISION)
    };
    if initialized == 0 {
        return Err(Error::last_os_error());
    }
    // SAFETY: `descriptor` and `acl` remain live, the DACL is present, and it
    // is explicitly marked as not defaulted.
    let attached = unsafe { SetSecurityDescriptorDacl((&raw mut descriptor).cast(), 1, acl, 0) };
    if attached == 0 {
        return Err(Error::last_os_error());
    }

    let path = wide_path(path)?;
    // SAFETY: the path is NUL-terminated and the initialized descriptor and
    // its attached ACL remain live for this non-retaining call.
    let applied = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION,
            (&raw mut descriptor).cast(),
        )
    };
    if applied == 0 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Reads the self-relative DACL security descriptor bytes for a file.
pub(crate) fn read_dacl_bytes(path: &Path) -> Result<Vec<u8>> {
    let path = wide_path(path)?;
    let mut needed = 0_u32;
    // SAFETY: the path remains live, a null zero-length output is the
    // documented size query, and `needed` is writable.
    let _ = unsafe {
        GetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };
    if needed == 0 {
        return Err(Error::last_os_error());
    }
    let mut storage = aligned_storage(needed)?;
    // SAFETY: `storage` is aligned and writable for at least `needed` bytes,
    // and the path and size pointer remain live for this non-retaining call.
    let read = unsafe {
        GetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION,
            storage.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    };
    if read == 0 {
        return Err(Error::last_os_error());
    }
    // SAFETY: the returned descriptor initialized exactly `needed` bytes in
    // the aligned allocation, which remains live while the slice is copied.
    let bytes =
        unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), needed as usize) };
    Ok(bytes.to_vec())
}

/// Builds the path syntax for a named alternate data stream.
pub(crate) fn alternate_data_stream_path(path: &Path, stream_name: &str) -> PathBuf {
    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    units.push(u16::from(b':'));
    units.extend(stream_name.encode_utf16());
    PathBuf::from(OsString::from_wide(&units))
}

/// Clears the Windows read-only attribute through the native attribute API.
pub(crate) fn clear_readonly_attribute(path: &Path) -> Result<()> {
    let path = wide_path(path)?;
    // SAFETY: the path is NUL-terminated and remains live for both
    // non-retaining attribute calls.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(Error::last_os_error());
    }
    // SAFETY: the path remains live and the flags came from the same entry,
    // with only the documented read-only bit cleared.
    let updated =
        unsafe { SetFileAttributesW(path.as_ptr(), attributes & !FILE_ATTRIBUTE_READONLY) };
    if updated == 0 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Allocates pointer-aligned zeroed storage for a Windows native buffer.
fn aligned_storage(byte_len: u32) -> Result<Vec<usize>> {
    let word_size = size_of::<usize>();
    let byte_len = byte_len as usize;
    let words = byte_len
        .checked_add(word_size - 1)
        .map(|length| length / word_size)
        .ok_or_else(|| Error::other("Windows native buffer size overflow"))?;
    Ok(vec![0_usize; words])
}

/// Converts a path to an interior-NUL-free Windows string.
fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path contains an interior NUL: {}", path.display()),
        ));
    }
    Ok(units.into_iter().chain(Some(0)).collect())
}
