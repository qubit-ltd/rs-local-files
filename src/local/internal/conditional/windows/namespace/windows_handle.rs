// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle operations used by the conditional namespace.

use std::fs::File;
use std::io;
use std::io::ErrorKind;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE;
use windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo;
use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::FileIdentity;

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

/// Reads the volume and file identifier associated with an open Windows handle.
///
/// # Parameters
///
/// - `file`: Open handle whose filesystem identity is read.
///
///
/// # Returns
///
/// The volume serial number and file identifier observed on the handle.
/// # Errors
///
/// Returns an error if Windows cannot read the identifier from the open handle.
pub(in crate::local::internal::conditional::windows) fn file_identity(file: &File) -> io::Result<FileIdentity> {
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: `file` owns a live handle and `identity` is a correctly sized
    // writable buffer for FileIdInfo.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&raw mut identity).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(FileIdentity {
        volume_serial: identity.VolumeSerialNumber,
        file_id: identity.FileId.Identifier,
    })
}

/// Rejects directories and reparse points when a regular file is required.
///
/// # Parameters
///
/// - `file`: Open handle to validate as a regular non-reparse file.
///
/// # Errors
///
/// Returns an error if metadata or Windows attributes cannot be read, or the
/// handle is not a regular non-reparse file.
pub(super) fn verify_regular_no_reparse(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "conditional entry is not a regular file",
        ));
    }
    let mut tag = windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `file` owns a live handle and `tag` is a correctly sized output
    // buffer.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo,
            (&raw mut tag).cast(),
            std::mem::size_of::<windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "conditional entry is a reparse point or directory",
        ));
    }
    Ok(())
}

/// Opens a host file without following a reparse point and verifies its type.
///
/// # Parameters
///
/// - `path`: Host path to open.
/// - `access`: Requested Windows access mask.
///
///
/// # Returns
///
/// The opened regular file handle.
/// # Errors
///
/// Returns an error if the path contains an interior NUL, opening fails, or the
/// handle is not a regular non-reparse file.
pub(super) fn open_host_file(path: &Path, access: u32) -> io::Result<File> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "path contains an interior NUL"));
    }
    // SAFETY: `wide` is NUL terminated and remains live for the synchronous
    // CreateFileW call. The returned handle is checked before ownership transfer.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            SHARE_ALL,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a uniquely owned valid handle.
    let file = unsafe { File::from_raw_handle(handle) };
    verify_regular_no_reparse(&file)?;
    Ok(file)
}

/// Opens a host directory without following reparse points and validates it.
///
/// # Parameters
///
/// - `path`: Host directory path to open and validate.
///
///
/// # Returns
///
/// The opened directory handle.
/// # Errors
///
/// Returns an error if the path contains an interior NUL, opening fails, or the
/// handle is not a real directory.
pub(super) fn open_host_directory_no_reparse(path: &Path) -> io::Result<File> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "path contains an interior NUL"));
    }
    // SAFETY: `wide` remains live through the synchronous call; the returned
    // handle is validated before ownership transfers to `File`.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
            SHARE_ALL,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a uniquely owned valid handle.
    let file = unsafe { File::from_raw_handle(handle) };
    verify_directory_no_reparse(&file)?;
    Ok(file)
}

/// Confirms an open handle refers to a directory rather than a reparse point.
///
/// # Parameters
///
/// - `file`: Open handle to validate as a real directory.
///
/// # Errors
///
/// Returns an error if Windows attributes cannot be read or the handle is not a
/// real directory.
pub(super) fn verify_directory_no_reparse(file: &File) -> io::Result<()> {
    let mut tag = windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `file` owns a live handle and `tag` is a correctly sized output
    // buffer.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            std::mem::size_of_val(&tag) as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "conditional recovery container is not a real directory",
        ));
    }
    Ok(())
}

/// Marks an already-open entry for deletion through its handle.
///
/// # Parameters
///
/// - `file`: Open handle, previously granted delete access, to mark for
///   deletion.
///
/// # Errors
///
/// Returns an error if Windows cannot mark the open handle for deletion.
pub(super) fn delete_open_entry(file: &File) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileDispositionInfo;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: The handle was opened with DELETE and the buffer matches the info
    // class.
    let success = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
