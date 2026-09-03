// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Open-handle primitives for rooted Windows traversal.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT;
use windows_sys::Wdk::Storage::FileSystem::FILE_OVERWRITE_IF;
use windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::OBJ_CASE_INSENSITIVE;
use windows_sys::Win32::Foundation::UNICODE_STRING;
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::Storage::FileSystem::FILE_APPEND_DATA;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::super::OwnedUnicodeString;
use super::directory::create_rooted_directory;
use super::directory::verify_not_name_surrogate;
use super::directory::verify_real_directory;
use crate::local::LocalRelativePath;
use crate::read;
use crate::write;

/// Access shared by synchronous relative opens.
const ROOTED_SHARE_MODE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Opens an absolute root directory using ordinary final reparse-point
/// semantics exactly once during construction.
///
/// # Errors
///
/// Returns an I/O error when the root cannot be opened or is not a real
/// directory.
pub(crate) fn open_root_directory(path: &Path) -> Result<File> {
    let wide = wide_path(path)?;
    // SAFETY: `wide` is a live NUL-terminated UTF-16 path. All optional
    // pointers are null and the returned handle is validated before ownership
    // is transferred to `File`.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            ROOTED_SHARE_MODE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(Error::last_os_error());
    }
    // SAFETY: `handle` is valid and uniquely owned after CreateFileW.
    let directory = unsafe { File::from_raw_handle(handle) };
    verify_real_directory(&directory)?;
    Ok(directory)
}

/// Reads metadata for a rooted entry without following the final component.
///
/// # Errors
///
/// Returns an I/O error when traversal, opening, or metadata inspection fails.
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn read_rooted_symlink_metadata(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<File> {
    open_entry_no_follow(root, path, FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)
}

/// Opens a rooted regular file for reading.
///
/// # Errors
///
/// Returns an I/O error when traversal escapes through a reparse point or the
/// final entry is not a regular file.
pub(crate) fn open_rooted_native_reader(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
    _options: &read::OpenOptions,
) -> Result<File> {
    let file = open_entry(
        root,
        path,
        GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_NON_DIRECTORY_FILE,
    )?;
    verify_not_name_surrogate(&file)?;
    Ok(file)
}

/// Opens a rooted regular file for writing.
///
/// # Errors
///
/// Returns an I/O error when parent traversal, creation, or final verification
/// fails.
pub(crate) fn open_rooted_native_writer(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
    options: &write::OpenOptions,
) -> Result<File> {
    if options.creates_parents()
        && let Some(parent) = path.as_path().parent().filter(|parent| !parent.as_os_str().is_empty())
    {
        create_rooted_directory(root, Path::new(""), &LocalRelativePath::new(parent)?, true, true)?;
    }
    let (access, disposition) = match options.mode() {
        write::Mode::CreateOrTruncate => (GENERIC_WRITE, FILE_OVERWRITE_IF),
        write::Mode::CreateNew => (GENERIC_WRITE, FILE_CREATE),
        write::Mode::OpenExistingAtStart => (GENERIC_WRITE, FILE_OPEN),
        write::Mode::AppendExisting => (FILE_APPEND_DATA, FILE_OPEN),
        write::Mode::AppendOrCreate => (FILE_APPEND_DATA, FILE_OPEN_IF),
    };
    let file = open_entry(
        root,
        path,
        access | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        disposition,
        FILE_NON_DIRECTORY_FILE,
    )?;
    verify_not_name_surrogate(&file)?;
    Ok(file)
}

/// Opens one validated rooted entry after securely opening every parent.
pub(super) fn open_entry(
    root: &File,
    path: &LocalRelativePath,
    access: u32,
    disposition: u32,
    options: u32,
) -> Result<File> {
    let entry = open_entry_no_follow(root, path, access, disposition, options)?;
    verify_not_name_surrogate(&entry)?;
    Ok(entry)
}

/// Opens one rooted entry without following or rejecting its final reparse
/// point.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(super) fn open_entry_no_follow(
    root: &File,
    path: &LocalRelativePath,
    access: u32,
    disposition: u32,
    options: u32,
) -> Result<File> {
    let (parent, name) = open_parent(root, path)?;
    nt_open_at(&parent, &name, access, disposition, options)
}

/// Converts a native path to a NUL-terminated UTF-16 string.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
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

/// Opens and verifies every parent component beneath the root.
pub(super) fn open_parent(root: &File, path: &LocalRelativePath) -> Result<(File, OsString)> {
    let mut components: Vec<OsString> = path
        .as_path()
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    let name = components
        .pop()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rooted path is empty"))?;
    let mut parent = root.try_clone()?;
    for component in components {
        let directory = nt_open_at(
            &parent,
            &component,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE,
        )?;
        verify_real_directory(&directory)?;
        parent = directory;
    }
    Ok((parent, name))
}

/// Opens one name relative to an already opened directory handle.
pub(super) fn nt_open_at(parent: &File, name: &OsStr, access: u32, disposition: u32, options: u32) -> Result<File> {
    let name = unicode_string(name)?;
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &raw const name.header,
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
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
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
/// Builds one NT counted Unicode string without a trailing NUL.
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
    Ok(OwnedUnicodeString { _units: units, header })
}
/// Converts an NTSTATUS result into a standard I/O result.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(super) fn nt_result(status: i32) -> Result<()> {
    if status >= 0 {
        return Ok(());
    }
    // SAFETY: RtlNtStatusToDosErrorNoTeb accepts any NTSTATUS value and does
    // not retain pointers.
    let code = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    Err(Error::from_raw_os_error(code as i32))
}
