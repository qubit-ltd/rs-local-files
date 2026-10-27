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
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_INFORMATION;
use windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_POSIX_SEMANTICS;
use windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_REPLACE_IF_EXISTS;
use windows_sys::Wdk::Storage::FileSystem::FileRenameInformation;
use windows_sys::Wdk::Storage::FileSystem::FileRenameInformationEx;
use windows_sys::Wdk::Storage::FileSystem::NtSetInformationFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Win32::Foundation::STATUS_INVALID_INFO_CLASS;
use windows_sys::Win32::Foundation::STATUS_INVALID_PARAMETER;
use windows_sys::Win32::Foundation::STATUS_NOT_SUPPORTED;
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
#[inline]
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

/// Marks the entry identified by an opened handle for deletion on close.
/// The caller must have opened `entry` with DELETE access. Returns the native
/// disposition error; success does not itself close the supplied handle.
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

/// Deletes an owned entry while preserving its captured identity and the
/// original file attributes when compatibility fallback deletion fails.
pub(super) fn delete_owned_read_only_entry(entry: &File, expected: &crate::rooted::Metadata) -> Result<()> {
    let actual = crate::rooted::Metadata::from_open_file(entry)?;
    if !expected.is_same_file(&actual) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "owned rooted temporary entry identity changed",
        ));
    }

    match delete_with_extended_disposition(entry, true) {
        Ok(()) => Ok(()),
        Err(error) if extended_disposition_is_unsupported(&error) => delete_with_attribute_fallback(entry),
        Err(error) => Err(error),
    }
}

/// Requests extended deletion, optionally asking Windows to ignore read-only.
fn delete_with_extended_disposition(entry: &File, ignore_read_only: bool) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_FLAG_DELETE;
    use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE;
    use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO_EX;
    use windows_sys::Win32::Storage::FileSystem::FileDispositionInfoEx;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

    #[cfg(feature = "test-support")]
    if ignore_read_only
        && (crate::local::take_test_support("rooted-owned-delete-fallback-delete-once")
            || crate::local::take_test_support("rooted-owned-delete-fallback-success")
            || crate::local::take_test_support("rooted-owned-delete-restore-fail"))
    {
        return Err(Error::from_raw_os_error(
            windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED as i32,
        ));
    }

    let flags = if ignore_read_only {
        FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE
    } else {
        FILE_DISPOSITION_FLAG_DELETE
    };
    let disposition = FILE_DISPOSITION_INFO_EX { Flags: flags };
    // SAFETY: `entry` is a live handle opened with DELETE access and the
    // payload matches the FileDispositionInfoEx information class.
    let result = unsafe {
        SetFileInformationByHandle(
            entry.as_raw_handle(),
            FileDispositionInfoEx,
            (&raw const disposition).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Uses the legacy disposition only after the extended class is explicitly
/// unavailable. Restores every basic attribute field if deletion fails.
fn delete_with_attribute_fallback(entry: &File) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
    use windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

    let mut original = FILE_BASIC_INFO::default();
    // SAFETY: `entry` is live and `original` is writable storage for
    // FileBasicInfo.
    let result = unsafe {
        GetFileInformationByHandleEx(
            entry.as_raw_handle(),
            FileBasicInfo,
            (&raw mut original).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(Error::last_os_error());
    }

    if original.FileAttributes & FILE_ATTRIBUTE_READONLY == 0 {
        return delete_open_entry(entry);
    }

    let mut writable = original;
    writable.FileAttributes &= !FILE_ATTRIBUTE_READONLY;
    if writable.FileAttributes == 0 {
        writable.FileAttributes = FILE_ATTRIBUTE_NORMAL;
    }
    // SAFETY: `entry` was opened with FILE_WRITE_ATTRIBUTES and `writable`
    // matches the FileBasicInfo information class.
    let result = unsafe {
        SetFileInformationByHandle(
            entry.as_raw_handle(),
            FileBasicInfo,
            (&raw const writable).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(Error::last_os_error());
    }

    #[cfg(feature = "test-support")]
    let delete_result = if crate::local::test_support_enabled("rooted-owned-delete-fallback-delete-once")
        || crate::local::test_support_enabled("rooted-owned-delete-restore-fail")
    {
        Err(crate::local::test_fault_error())
    } else {
        delete_after_readonly_clear(entry)
    };
    #[cfg(not(feature = "test-support"))]
    let delete_result = delete_after_readonly_clear(entry);

    match delete_result {
        Ok(()) => Ok(()),
        Err(delete_error) => {
            #[cfg(feature = "test-support")]
            let restore_result = if crate::local::test_support_enabled("rooted-owned-delete-restore-fail") {
                Err(crate::local::test_fault_error())
            } else {
                restore_file_basic_info(entry, &original)
            };
            #[cfg(not(feature = "test-support"))]
            let restore_result = restore_file_basic_info(entry, &original);

            match restore_result {
                Ok(()) => Err(delete_error),
                Err(restore_error) => Err(Error::new(
                    delete_error.kind(),
                    format!(
                        "deletion failed: {delete_error}; restoring original file attributes failed: {restore_error}"
                    ),
                )),
            }
        }
    }
}

/// Retries deletion without the read-only override before using the legacy
/// disposition class on filesystems that reject the extended class.
fn delete_after_readonly_clear(entry: &File) -> Result<()> {
    match delete_with_extended_disposition(entry, false) {
        Err(error) if extended_disposition_is_unsupported(&error) => delete_open_entry(entry),
        result => result,
    }
}

/// Restores the complete captured `FILE_BASIC_INFO` payload.
fn restore_file_basic_info(
    entry: &File,
    original: &windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

    // SAFETY: `entry` is live and `original` matches the FileBasicInfo class.
    let result = unsafe {
        SetFileInformationByHandle(
            entry.as_raw_handle(),
            FileBasicInfo,
            (&raw const *original).cast(),
            size_of::<windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO>() as u32,
        )
    };
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Returns whether the extended disposition failure means the class is
/// unsupported, rather than an access, sharing, or namespace failure.
fn extended_disposition_is_unsupported(error: &Error) -> bool {
    use windows_sys::Win32::Foundation::ERROR_CALL_NOT_IMPLEMENTED;
    use windows_sys::Win32::Foundation::ERROR_INVALID_FUNCTION;
    use windows_sys::Win32::Foundation::ERROR_INVALID_LEVEL;
    use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
    use windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED;

    matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(ERROR_CALL_NOT_IMPLEMENTED)
            | Some(ERROR_INVALID_FUNCTION)
            | Some(ERROR_INVALID_LEVEL)
            | Some(ERROR_INVALID_PARAMETER)
            | Some(ERROR_NOT_SUPPORTED)
    )
}

/// Renames an opened entry relative to the same root handle.
/// Opens the source without following its final link and retains the target
/// parent during installation. `overwrite` permits replacement, with a legacy
/// fallback when the extended information class is unsupported. Propagates
/// native traversal/open/rename and payload-construction errors.
pub(super) fn rename_open_entry(
    root: &File,
    source: &LocalRelativePath,
    destination: &LocalRelativePath,
    overwrite: bool,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::DELETE;
    let source = open_entry_no_follow(root, source, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
    let (destination_parent, destination_name) = open_parent_for_rename(root, destination, overwrite)?;
    let (mut buffer, information_length) = build_rename_information(destination_name.as_os_str(), overwrite)?;
    // SAFETY: `Vec<usize>` provides alignment suitable for the native
    // FILE_RENAME_INFORMATION payload.
    let information = unsafe { &mut *buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>() };
    information.RootDirectory = destination_parent.as_raw_handle();
    if overwrite {
        // The POSIX rename mode allows an existing destination to be replaced
        // atomically while preserving handle-relative traversal.
        information.Anonymous.Flags = FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
    }
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: `source` and the destination parent remain open, `buffer` is a
    // complete FILE_RENAME_INFORMATION payload, and the native call does not
    // retain any pointer after returning.
    let mut status = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &raw mut status_block,
            buffer.as_ptr().cast(),
            information_length,
            if overwrite {
                FileRenameInformationEx
            } else {
                FileRenameInformation
            },
        )
    };
    if overwrite && is_legacy_rename_status(status) {
        // Older Windows filesystems may reject the extended information
        // class. Retry with the original replacement contract.
        information.Anonymous.ReplaceIfExists = true;
        status = unsafe {
            NtSetInformationFile(
                source.as_raw_handle(),
                &raw mut status_block,
                buffer.as_ptr().cast(),
                information_length,
                FileRenameInformation,
            )
        };
    }
    if status < 0 {
        let error = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
        Err(Error::from_raw_os_error(error as i32))
    } else {
        Ok(())
    }
}

/// Renames one entry between retained Windows directory handles.
#[cfg(feature = "digest")]
pub(in crate::local::internal) fn rename_conditional_entry_at(
    source_parent: &File,
    source_name: &OsStr,
    destination_parent: &File,
    destination_name: &OsStr,
    overwrite: bool,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::DELETE;

    let source = super::native::nt_open_at(
        source_parent,
        source_name,
        DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        0,
    )?;
    let (mut buffer, information_length) = build_rename_information(destination_name, overwrite)?;
    // SAFETY: the aligned buffer contains a complete rename payload and both
    // retained parent handles remain open throughout the native call.
    let information = unsafe { &mut *buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>() };
    information.RootDirectory = destination_parent.as_raw_handle();
    if overwrite {
        information.Anonymous.Flags = FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
    }
    let mut status_block = IO_STATUS_BLOCK::default();
    let mut status = unsafe {
        NtSetInformationFile(
            source.as_raw_handle(),
            &raw mut status_block,
            buffer.as_ptr().cast(),
            information_length,
            if overwrite {
                FileRenameInformationEx
            } else {
                FileRenameInformation
            },
        )
    };
    if overwrite && is_legacy_rename_status(status) {
        information.Anonymous.ReplaceIfExists = true;
        status = unsafe {
            NtSetInformationFile(
                source.as_raw_handle(),
                &raw mut status_block,
                buffer.as_ptr().cast(),
                information_length,
                FileRenameInformation,
            )
        };
    }
    if status < 0 {
        // SAFETY: conversion accepts any NTSTATUS and retains no pointers.
        Err(Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosErrorNoTeb(status) } as i32
        ))
    } else {
        Ok(())
    }
}

/// Builds the variable-sized `FILE_RENAME_INFORMATION` payload used by NT.
///
/// The returned length includes the complete UTF-16 name storage required by
/// `NtSetInformationFile`, while the boolean controls replacement of an
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
        .checked_sub(size_of::<u16>())
        .and_then(|header| header.checked_add(file_name_bytes))
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rename buffer is too large"))?;
    let information_length = u32::try_from(
        size_of::<FILE_RENAME_INFORMATION>()
            .checked_sub(size_of::<u16>())
            .and_then(|header| header.checked_add(file_name_bytes))
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

/// Returns whether the extended rename information class is unavailable on
/// the current filesystem and the legacy class is an appropriate fallback.
fn is_legacy_rename_status(status: i32) -> bool {
    matches!(
        status,
        STATUS_INVALID_INFO_CLASS | STATUS_INVALID_PARAMETER | STATUS_NOT_SUPPORTED
    )
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_INFORMATION;

    use super::STATUS_INVALID_INFO_CLASS;
    use super::STATUS_INVALID_PARAMETER;
    use super::STATUS_NOT_SUPPORTED;
    use super::build_rename_information;
    use super::is_legacy_rename_status;
    use crate::local::LocalRelativePath;

    /// Verifies rooted rename buffers include all UTF-16 filename bytes.
    #[test]
    fn test_rename_buffer_reports_complete_payload_length() {
        let destination = LocalRelativePath::new(Path::new("nested/renamed")).expect("destination should be valid");
        let (buffer, information_length) =
            build_rename_information(destination.as_path().as_os_str(), true).expect("rename payload should build");
        let information = unsafe { &*buffer.as_ptr().cast::<FILE_RENAME_INFORMATION>() };
        let expected_name_bytes = destination.as_path().as_os_str().encode_wide().count() * size_of::<u16>();

        assert_eq!(
            information_length as usize,
            size_of::<FILE_RENAME_INFORMATION>() - size_of::<u16>() + expected_name_bytes
        );
        assert_eq!(information.FileNameLength as usize, expected_name_bytes);
        assert!(unsafe { information.Anonymous.ReplaceIfExists });
    }

    #[test]
    fn test_legacy_rename_fallback_is_limited_to_unsupported_classes() {
        assert!(is_legacy_rename_status(STATUS_INVALID_INFO_CLASS));
        assert!(is_legacy_rename_status(STATUS_INVALID_PARAMETER));
        assert!(is_legacy_rename_status(STATUS_NOT_SUPPORTED));
        assert!(!is_legacy_rename_status(-1073741790));
    }
}
