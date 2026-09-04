// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Handle-relative Windows namespace mutation primitives.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::mem::offset_of;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_INFORMATION;
use windows_sys::Wdk::Storage::FileSystem::FileRenameInformation;
use windows_sys::Wdk::Storage::FileSystem::NtSetInformationFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::handle::open_entry;
use super::handle::open_entry_no_follow;
use super::handle::open_parent_for_rename;
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
    let source = open_entry_no_follow(root, source, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
    let _destination = if overwrite {
        match open_entry_no_follow(
            root,
            destination,
            DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            0,
        ) {
            Ok(destination) => Some(destination),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let (destination_parent, destination_name) = open_parent_for_rename(root, destination)?;
    let (mut buffer, information_length) = build_rename_information(destination_name.as_os_str(), overwrite)?;
    // SAFETY: `Vec<usize>` provides alignment suitable for the native
    // FILE_RENAME_INFORMATION payload.
    let information = unsafe { &mut *buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>() };
    information.RootDirectory = destination_parent.as_raw_handle();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: `source` and the destination parent remain open, `buffer` is a
    // complete FILE_RENAME_INFORMATION payload, and the native call does not
    // retain any pointer after returning.
    let status = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &raw mut status_block,
            buffer.as_ptr().cast(),
            information_length,
            FileRenameInformation,
        )
    };
    if status < 0 {
        let error = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
        Err(Error::from_raw_os_error(error as i32))
    } else {
        Ok(())
    }
}

/// Builds the variable-sized `FILE_RENAME_INFO` payload required by Windows.
///
/// The returned length includes the complete UTF-16 name storage required by
/// `SetFileInformationByHandle`, while the boolean controls replacement of an
/// existing destination.
///
/// # Errors
///
/// Returns `InvalidInput` if the UTF-16 name or complete payload length cannot
/// be represented by the Windows API.
fn build_rename_information(destination: &OsStr, overwrite: bool) -> Result<(Vec<usize>, u32)> {
    use windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_INFORMATION;

    let destination_units: Vec<u16> = destination.encode_wide().collect();
    let file_name_bytes = destination_units
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rename name is too long"))?;
    let allocation = size_of::<FILE_RENAME_INFORMATION>()
        .checked_add(file_name_bytes)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rename buffer is too large"))?;
    let information_length = u32::try_from(
        offset_of!(FILE_RENAME_INFORMATION, FileName)
            .checked_add(file_name_bytes)
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rename buffer is too large"))?,
    )
    .map_err(|_| Error::new(ErrorKind::InvalidInput, "rename buffer is too large"))?;
    let mut buffer = vec![0_usize; allocation.div_ceil(size_of::<usize>())];
    // SAFETY: `Vec<usize>` provides alignment suitable for
    // `FILE_RENAME_INFORMATION`,
    // and the allocation includes the complete trailing UTF-16 name.
    let information = unsafe { &mut *buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>() };
    information.Anonymous.ReplaceIfExists = overwrite;
    information.FileNameLength =
        u32::try_from(file_name_bytes).map_err(|_| Error::new(ErrorKind::InvalidInput, "rename name is too long"))?;
    // SAFETY: the allocation reserves enough trailing storage for the full
    // destination name and the source slice remains live for the copy.
    unsafe {
        std::ptr::copy_nonoverlapping(
            destination_units.as_ptr(),
            information.FileName.as_mut_ptr(),
            destination_units.len(),
        );
    }
    Ok((buffer, information_length))
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_INFORMATION;

    use super::build_rename_information;
    use crate::local::LocalRelativePath;

    /// Verifies rooted rename buffers include all UTF-16 filename bytes.
    #[test]
    fn rename_buffer_reports_complete_payload_length() {
        let destination = LocalRelativePath::new(Path::new("nested/renamed")).expect("destination should be valid");
        let (buffer, information_length) =
            build_rename_information(destination.as_path().as_os_str(), true).expect("rename payload should build");
        let information = unsafe { &*buffer.as_ptr().cast::<FILE_RENAME_INFORMATION>() };
        let expected_name_bytes = destination.as_path().as_os_str().encode_wide().count() * size_of::<u16>();

        assert_eq!(
            information_length as usize,
            std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName) + expected_name_bytes
        );
        assert_eq!(information.FileNameLength as usize, expected_name_bytes);
        assert!(unsafe { information.Anonymous.ReplaceIfExists });
    }
}
