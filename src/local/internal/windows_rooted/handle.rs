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
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE;
use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::native::ROOTED_SHARE_MODE;
use super::native::nt_open_at;
use super::native::verify_not_name_surrogate;
use super::native::verify_real_directory;
use crate::local::LocalRelativePath;
use crate::read;

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
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
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
#[inline]
pub(crate) fn read_rooted_symlink_metadata(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<File> {
    open_entry_no_follow(root, path, FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)
}

/// Opens one child entry relative to an already-opened directory handle,
/// retaining the final reparse point for classification by the caller.
/// `name` must be one previously validated normal component. Propagates
/// name-encoding and native open errors; the returned handle owns the entry.
pub(crate) fn read_rooted_component_metadata(root: &File, name: &OsStr) -> Result<File> {
    nt_open_at(root, name, FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)
}

/// Opens and verifies one real child directory relative to an already-opened
/// directory handle without following name-surrogate reparse points.
/// `name` must be one previously validated normal component. Propagates native
/// open/inspection, non-directory, reparse-point, and name-encoding errors.
pub(crate) fn open_rooted_component_directory(root: &File, name: &OsStr) -> Result<File> {
    let directory = nt_open_at(
        root,
        name,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE,
    )?;
    verify_real_directory(&directory)?;
    Ok(directory)
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

/// Opens one validated rooted entry after securely opening every parent.
/// Forwards native `access`, `disposition`, and `options` to the final open,
/// which may create or truncate according to `disposition`. Rejects final
/// name-surrogate reparse points and propagates traversal/open/inspection
/// errors.
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
/// Securely opens each parent and returns an owned final handle using the
/// requested `access`, `disposition`, and `options`. Propagates traversal,
/// name-encoding, and native open errors; creation/truncation is not rolled
/// back.
#[inline]
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
/// Returns `InvalidInput` for an embedded native NUL.
#[inline]
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
/// Returns the owned parent handle and final native name, propagating handle
/// duplication, traversal, name-encoding, and directory verification errors.
pub(super) fn open_parent(root: &File, path: &LocalRelativePath) -> Result<(File, OsString)> {
    open_parent_with_access(
        root,
        path,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
    )
}

/// Opens every parent component with the rights needed for a rooted rename.
/// Uses [`open_parent`]'s result and error contract. `overwrite` currently
/// does not change the required parent access rights.
pub(super) fn open_parent_for_rename(
    root: &File,
    path: &LocalRelativePath,
    overwrite: bool,
) -> Result<(File, OsString)> {
    let _ = overwrite;
    open_parent(root, path)
}

/// Opens and verifies every parent component with the requested directory
/// rights.
/// Returns the owned parent and final name; an empty path is `InvalidInput`.
/// Native duplication/open/inspection and name-encoding failures propagate,
/// dropping any intermediate handles acquired by this attempt.
fn open_parent_with_access(root: &File, path: &LocalRelativePath, access: u32) -> Result<(File, OsString)> {
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
        let directory = nt_open_at(&parent, &component, access, FILE_OPEN, FILE_DIRECTORY_FILE)?;
        verify_real_directory(&directory)?;
        parent = directory;
    }
    Ok((parent, name))
}
