// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative rooted filesystem operations.
// qubit-style: allow source-test-pair
// Platform behavior is covered through public rooted integration tests.

use std::ffi::{
    OsStr,
    OsString,
};
use std::fs::File;
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::mem::size_of;
use std::os::windows::ffi::{
    OsStrExt,
    OsStringExt,
};
use std::os::windows::io::{
    AsRawHandle,
    FromRawHandle,
};
use std::path::Path;
use std::ptr::{
    null,
    null_mut,
};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE,
    FILE_DIRECTORY_FILE,
    FILE_DIRECTORY_INFORMATION,
    FILE_NON_DIRECTORY_FILE,
    FILE_OPEN,
    FILE_OPEN_IF,
    FILE_OPEN_REPARSE_POINT,
    FILE_OVERWRITE_IF,
    FILE_SYNCHRONOUS_IO_NONALERT,
    FileDirectoryInformation,
    NtCreateFile,
    NtQueryDirectoryFile,
    RtlNtStatusToDosErrorNoTeb,
};
use windows_sys::Win32::Foundation::{
    GENERIC_READ,
    GENERIC_WRITE,
    HANDLE,
    INVALID_HANDLE_VALUE,
    OBJ_CASE_INSENSITIVE,
    STATUS_NO_MORE_FILES,
    UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW,
    FILE_APPEND_DATA,
    FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL,
    FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE,
    FILE_SHARE_READ,
    FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES,
    FileAttributeTagInfo,
    GetFileInformationByHandleEx,
    OPEN_EXISTING,
    SYNCHRONIZE,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use crate::local::LocalRelativePath;
use crate::{
    read,
    write,
};

/// Reparse-tag bit identifying name-surrogate entries.
const IO_REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;
/// Access shared by synchronous relative opens.
const ROOTED_SHARE_MODE: u32 =
    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Opens an absolute root directory without following its final reparse point.
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
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
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
pub(crate) fn read_rooted_symlink_metadata(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<File> {
    open_entry_no_follow(
        root,
        path,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        0,
    )
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
        && let Some(parent) = path
            .as_path()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_rooted_directory(
            root,
            Path::new(""),
            &LocalRelativePath::new(parent)?,
            true,
            true,
        )?;
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

/// Lists immediate children of the opened root.
///
/// # Errors
///
/// Returns an I/O error when enumeration or child inspection fails.
pub(crate) fn read_root_directory(
    root: &File,
    diagnostic_root: &Path,
) -> Result<Vec<(OsString, File)>> {
    read_directory_handle(root, diagnostic_root)
}

/// Lists immediate children of a rooted descendant directory.
///
/// # Errors
///
/// Returns an I/O error when traversal, enumeration, or child inspection
/// fails.
pub(crate) fn read_rooted_directory(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<Vec<(OsString, File)>> {
    let directory = open_entry(
        root,
        path,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE,
    )?;
    verify_real_directory(&directory)?;
    read_directory_handle(&directory, diagnostic_root)
}

/// Creates one rooted directory or directory chain.
///
/// # Errors
///
/// Returns an I/O error when secure traversal or creation fails.
pub(crate) fn create_rooted_directory(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
    recursive: bool,
    exists_ok: bool,
) -> Result<()> {
    let components: Vec<OsString> = path
        .as_path()
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    let mut parent = root.try_clone()?;
    for (index, component) in components.iter().enumerate() {
        let final_component = index + 1 == components.len();
        let disposition = if final_component {
            if exists_ok { FILE_OPEN_IF } else { FILE_CREATE }
        } else if recursive {
            FILE_OPEN_IF
        } else {
            FILE_OPEN
        };
        match nt_open_at(
            &parent,
            component,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            disposition,
            FILE_DIRECTORY_FILE,
        ) {
            Ok(directory) => {
                verify_real_directory(&directory)?;
                parent = directory;
            }
            Err(source_error)
                if !recursive
                    && !final_component
                    && source_error.kind() == ErrorKind::NotFound =>
            {
                return Err(source_error);
            }
            Err(source_error) => return Err(source_error),
        }
    }
    Ok(())
}

/// Removes one rooted entry or directory tree without following reparse points.
///
/// # Errors
///
/// Returns an I/O error when traversal, enumeration, or handle deletion fails.
pub(crate) fn remove_rooted_entry(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    recursive: bool,
) -> Result<()> {
    let entry = read_rooted_symlink_metadata(root, diagnostic_root, path)?;
    if !entry.metadata()?.is_dir() || !recursive {
        return delete_rooted_entry(root, path);
    }

    let mut work = vec![(path.clone(), false)];
    while let Some((current, remove_directory)) = work.pop() {
        if remove_directory {
            delete_rooted_entry(root, &current)?;
            continue;
        }
        let entry =
            read_rooted_symlink_metadata(root, diagnostic_root, &current)?;
        if !entry.metadata()?.is_dir() {
            delete_rooted_entry(root, &current)?;
            continue;
        }
        work.push((current.clone(), true));
        for (name, _) in read_rooted_directory(root, diagnostic_root, &current)?
            .into_iter()
            .rev()
        {
            work.push((current.join_component(&name)?, false));
        }
    }
    Ok(())
}

/// Opens and deletes one rooted entry without following a reparse point.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be opened or deleted.
fn delete_rooted_entry(root: &File, path: &LocalRelativePath) -> Result<()> {
    let entry = open_entry_no_follow(
        root,
        path,
        windows_sys::Win32::Storage::FileSystem::DELETE
            | FILE_READ_ATTRIBUTES
            | SYNCHRONIZE,
        FILE_OPEN,
        0,
    )?;
    delete_open_entry(&entry)
}

/// Renames one rooted entry within the same opened root.
///
/// # Errors
///
/// Returns an I/O error when traversal or handle-relative rename fails.
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

/// Opens one validated rooted entry after securely opening every parent.
fn open_entry(
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
fn open_entry_no_follow(
    root: &File,
    path: &LocalRelativePath,
    access: u32,
    disposition: u32,
    options: u32,
) -> Result<File> {
    let (parent, name) = open_parent(root, path)?;
    nt_open_at(&parent, &name, access, disposition, options)
}

/// Opens and verifies every parent component beneath the root.
fn open_parent(
    root: &File,
    path: &LocalRelativePath,
) -> Result<(File, OsString)> {
    let mut components: Vec<OsString> = path
        .as_path()
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    let name = components.pop().ok_or_else(|| {
        Error::new(ErrorKind::InvalidInput, "rooted path is empty")
    })?;
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
fn nt_open_at(
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

/// Enumerates one already opened directory with `NtQueryDirectoryFile`.
fn read_directory_handle(
    directory: &File,
    _diagnostic_root: &Path,
) -> Result<Vec<(OsString, File)>> {
    let mut entries = Vec::new();
    let mut restart = true;
    loop {
        let buffer_size = 64_usize * 1024;
        let mut buffer =
            vec![0_usize; buffer_size.div_ceil(size_of::<usize>())];
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: `buffer` and `status_block` are writable for the duration of
        // the synchronous query. Optional event, APC, context, and filter
        // pointers are null.
        let status = unsafe {
            NtQueryDirectoryFile(
                directory.as_raw_handle(),
                null_mut(),
                None,
                null(),
                &raw mut status_block,
                buffer.as_mut_ptr().cast(),
                buffer_size as u32,
                FileDirectoryInformation,
                false,
                null(),
                restart,
            )
        };
        if status == STATUS_NO_MORE_FILES {
            break;
        }
        nt_result(status)?;
        restart = false;
        let used = status_block.Information.min(buffer_size);
        let buffer = buffer.as_ptr().cast::<u8>();
        let mut offset = 0_usize;
        while offset < used {
            // SAFETY: NtQueryDirectoryFile returned a sequence of
            // FILE_DIRECTORY_INFORMATION records within `used` bytes.
            let information = unsafe {
                &*buffer.add(offset).cast::<FILE_DIRECTORY_INFORMATION>()
            };
            let name_len =
                information.FileNameLength as usize / size_of::<u16>();
            // SAFETY: FileNameLength describes the inline UTF-16 name in this
            // record and the record lies inside the returned buffer.
            let name_units = unsafe {
                std::slice::from_raw_parts(
                    information.FileName.as_ptr(),
                    name_len,
                )
            };
            let name = OsString::from_wide(name_units);
            if name != OsStr::new(".") && name != OsStr::new("..") {
                let child = nt_open_at(
                    directory,
                    &name,
                    FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    FILE_OPEN,
                    0,
                )?;
                entries.push((name, child));
            }
            if information.NextEntryOffset == 0 {
                break;
            }
            offset = offset
                .checked_add(information.NextEntryOffset as usize)
                .ok_or_else(|| {
                Error::other("directory record offset overflowed")
            })?;
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

/// Rejects name-surrogate reparse points for an opened handle.
fn verify_not_name_surrogate(file: &File) -> Result<()> {
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

/// Verifies an opened handle is a real directory rather than a reparse point.
fn verify_real_directory(directory: &File) -> Result<()> {
    let attributes = handle_attributes(directory)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(Error::new(
            ErrorKind::NotADirectory,
            "rooted component is not a directory",
        ));
    }
    verify_not_name_surrogate(directory)
}

/// Reads file attributes and the reparse tag from an opened handle.
fn handle_attributes(file: &File) -> Result<FILE_ATTRIBUTE_TAG_INFO> {
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

/// Deletes the entry identified by an already opened handle.
fn delete_open_entry(entry: &File) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO,
        FileDispositionInfo,
        SetFileInformationByHandle,
    };

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
fn rename_open_entry(
    root: &File,
    source: &LocalRelativePath,
    destination: &LocalRelativePath,
    overwrite: bool,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE,
        FILE_RENAME_INFO,
        FileRenameInfo,
        SetFileInformationByHandle,
    };

    let source = open_entry_no_follow(
        root,
        source,
        DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        0,
    )?;
    let destination_units: Vec<u16> =
        destination.as_path().as_os_str().encode_wide().collect();
    let allocation = size_of::<FILE_RENAME_INFO>()
        .checked_add(
            destination_units.len().saturating_sub(1) * size_of::<u16>(),
        )
        .ok_or_else(|| {
            Error::new(ErrorKind::InvalidInput, "rename buffer is too large")
        })?;
    let mut buffer = vec![0_usize; allocation.div_ceil(size_of::<usize>())];
    // SAFETY: `Vec<usize>` provides alignment suitable for
    // `FILE_RENAME_INFO`, and the allocation includes the trailing UTF-16
    // name.
    let information =
        unsafe { &mut *buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>() };
    information.Anonymous.ReplaceIfExists = overwrite;
    information.RootDirectory = root.as_raw_handle();
    information.FileNameLength =
        u32::try_from(destination_units.len() * size_of::<u16>()).map_err(
            |_| Error::new(ErrorKind::InvalidInput, "rename name is too long"),
        )?;
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
            u32::try_from(allocation).map_err(|_| {
                Error::new(
                    ErrorKind::InvalidInput,
                    "rename buffer is too large",
                )
            })?,
        )
    };
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Owns a UTF-16 buffer and its borrowed `UNICODE_STRING` header.
struct OwnedUnicodeString {
    /// Stable UTF-16 storage referenced by `header`.
    _units: Vec<u16>,
    /// NT string header passed to object attributes.
    header: UNICODE_STRING,
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
        .ok_or_else(|| {
            Error::new(ErrorKind::InvalidInput, "rooted component is too long")
        })?;
    let header = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        Buffer: units.as_mut_ptr(),
    };
    Ok(OwnedUnicodeString {
        _units: units,
        header,
    })
}

/// Converts a native path to a NUL-terminated UTF-16 string.
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

/// Converts an NTSTATUS result into a standard I/O result.
fn nt_result(status: i32) -> Result<()> {
    if status >= 0 {
        return Ok(());
    }
    // SAFETY: RtlNtStatusToDosErrorNoTeb accepts any NTSTATUS value and does
    // not retain pointers.
    let code = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    Err(Error::from_raw_os_error(code as i32))
}
