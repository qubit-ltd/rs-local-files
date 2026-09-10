// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Low-level Windows NT rooted handle primitives.
// qubit-style: allow source-test-pair

use std::ffi::OsStr;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_FOR_BACKUP_INTENT;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT;
use windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::OBJ_CASE_INSENSITIVE;
use windows_sys::Win32::Foundation::UNICODE_STRING;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::super::OwnedUnicodeString;

/// Access shared by synchronous relative opens.
pub(super) const ROOTED_SHARE_MODE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
/// Reparse-tag bit identifying name-surrogate entries.
const IO_REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;

/// Opens one name relative to an already opened directory handle.
/// The caller must supply one validated normal `name` and native `access`,
/// `disposition`, and `options` consistent with the intended operation. Opens
/// synchronously without following the final reparse point; the disposition
/// can create or truncate. Returns encoding/native errors or an invalid-handle
/// error; success transfers the native handle into the returned `File`.
pub(in crate::local::internal) fn nt_open_at(
    parent: &File,
    name: &OsStr,
    access: u32,
    disposition: u32,
    options: u32,
) -> Result<File> {
    let name = unicode_string(name)?;
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: name.header(),
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let mut status_block = IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = null_mut();
    // SAFETY: all pointers refer to live stack values or the live UTF-16
    // buffer owned by `name`. `parent` remains open throughout the call and
    // NtCreateFile does not retain the object attributes.
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            access,
            &raw const attributes,
            &raw mut status_block,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            ROOTED_SHARE_MODE,
            disposition,
            options | FILE_OPEN_FOR_BACKUP_INTENT | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            null(),
            0,
        )
    };
    nt_result(status)?;
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(Error::other("NtCreateFile returned an invalid handle"));
    }
    // SAFETY: successful NtCreateFile returned a uniquely owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

/// Reads file attributes and the reparse tag from an opened handle.
/// Propagates the native `GetFileInformationByHandleEx` error.
pub(super) fn handle_attributes(file: &File) -> Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `file` owns a live handle and `attributes` is a correctly sized
    // writable buffer for FileAttributeTagInfo.
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut attributes).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(attributes)
    }
}

/// Rejects name-surrogate reparse points for an opened handle.
/// Returns `InvalidInput` for a name-surrogate tag or a native attribute-query
/// error; other reparse tags are accepted.
pub(super) fn verify_not_name_surrogate(file: &File) -> Result<()> {
    let attributes = handle_attributes(file)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && attributes.ReparseTag & IO_REPARSE_TAG_NAME_SURROGATE != 0
    {
        Err(Error::new(
            ErrorKind::InvalidInput,
            "rooted traversal rejected a name-surrogate reparse point",
        ))
    } else {
        Ok(())
    }
}

/// Verifies an opened handle is a directory without a name-surrogate tag.
/// Returns `NotADirectory` for a non-directory, `InvalidInput` for a forbidden
/// reparse point, or the native attribute-query error.
pub(super) fn verify_real_directory(directory: &File) -> Result<()> {
    let attributes = handle_attributes(directory)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(Error::new(
            ErrorKind::NotADirectory,
            "rooted component is not a directory",
        ));
    }
    verify_not_name_surrogate(directory)
}

/// Builds one NT counted Unicode string without a trailing NUL.
/// Returns `InvalidInput` for native NUL or a UTF-16 byte length exceeding
/// the native `u16` field. The returned value retains its backing storage.
fn unicode_string(value: &OsStr) -> Result<OwnedUnicodeString> {
    let mut units: Vec<u16> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "rooted component contains an interior NUL",
        ));
    }
    let byte_len = units
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rooted component is too long"))?;
    let header = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        Buffer: units.as_mut_ptr(),
    };
    Ok(OwnedUnicodeString::new(units, header))
}

/// Converts an NTSTATUS result into a standard I/O result.
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(in crate::local::internal) fn nt_result(status: i32) -> Result<()> {
    if status >= 0 {
        return Ok(());
    }
    // SAFETY: RtlNtStatusToDosErrorNoTeb accepts any NTSTATUS value and does
    // not retain pointers.
    let code = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    Err(Error::from_raw_os_error(code as i32))
}
