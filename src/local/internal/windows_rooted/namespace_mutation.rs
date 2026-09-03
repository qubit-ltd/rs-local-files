// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Handle-relative Windows namespace mutation primitives.

use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::handle::open_entry;
use super::handle::open_entry_no_follow;
use crate::local::LocalRelativePath;

/// Renames one rooted entry within the same opened root.
///
/// # Errors
///
/// Returns an I/O error when traversal or handle-relative rename fails.
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn rename_rooted_entry(
    root: &File,
    _diagnostic_root: &Path,
    source: &LocalRelativePath,
    destination: &LocalRelativePath,
    overwrite: bool,
) -> Result<()> {
    rename_open_entry(root, source, destination, overwrite)
}

/// Applies portable permissions to a rooted entry.
///
/// Windows currently maps the Unix write bits to the read-only attribute.
///
/// # Errors
///
/// Returns an I/O error when traversal, inspection, or attribute update fails.
pub(crate) fn set_rooted_permissions(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
    mode: u32,
) -> Result<()> {
    let entry = open_entry(
        root,
        path,
        FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        0,
    )?;
    let mut permissions = entry.metadata()?.permissions();
    permissions.set_readonly(mode & 0o222 == 0);
    entry.set_permissions(permissions)
}

/// Deletes the entry identified by an already opened handle.
pub(super) fn delete_open_entry(entry: &File) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileDispositionInfo;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `entry` was opened with DELETE access and `disposition` matches
    // FileDispositionInfo for the advertised buffer size.
    let result = unsafe {
        SetFileInformationByHandle(
            entry.as_raw_handle(),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Renames an opened entry relative to the same root handle.
pub(super) fn rename_open_entry(
    root: &File,
    source: &LocalRelativePath,
    destination: &LocalRelativePath,
    overwrite: bool,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::DELETE;
    use windows_sys::Win32::Storage::FileSystem::FILE_RENAME_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileRenameInfo;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

    let source = open_entry_no_follow(root, source, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
    let destination_units: Vec<u16> = destination.as_path().as_os_str().encode_wide().collect();
    let allocation = size_of::<FILE_RENAME_INFO>()
        .checked_add(destination_units.len().saturating_sub(1) * size_of::<u16>())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rename buffer is too large"))?;
    let mut buffer = vec![0_usize; allocation.div_ceil(size_of::<usize>())];
    // SAFETY: `Vec<usize>` provides alignment suitable for
    // `FILE_RENAME_INFO`, and the allocation includes the trailing UTF-16
    // name.
    let information = unsafe { &mut *buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>() };
    information.Anonymous.ReplaceIfExists = overwrite;
    information.RootDirectory = root.as_raw_handle();
    information.FileNameLength = u32::try_from(destination_units.len() * size_of::<u16>())
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "rename name is too long"))?;
    // SAFETY: the allocation reserves enough trailing storage for the full
    // destination name and the source slice remains live for the copy.
    unsafe {
        std::ptr::copy_nonoverlapping(
            destination_units.as_ptr(),
            information.FileName.as_mut_ptr(),
            destination_units.len(),
        );
    }
    // SAFETY: `source` is open with DELETE access and `buffer` contains a
    // complete FILE_RENAME_INFO structure for FileRenameInfo.
    let result = unsafe {
        SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfo,
            buffer.as_ptr().cast(),
            u32::try_from(allocation).map_err(|_| Error::new(ErrorKind::InvalidInput, "rename buffer is too large"))?,
        )
    };
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}
