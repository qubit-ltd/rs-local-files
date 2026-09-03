// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative rooted directory operations.
// qubit-style: allow source-test-pair

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_INFORMATION;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Wdk::Storage::FileSystem::FileDirectoryInformation;
use windows_sys::Wdk::Storage::FileSystem::NtQueryDirectoryFile;
use windows_sys::Win32::Foundation::STATUS_NO_MORE_FILES;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::handle::handle_attributes;
use super::handle::nt_open_at;
use super::handle::nt_result;
use super::handle::open_entry;
use super::handle::open_entry_no_follow;
use super::handle::read_rooted_symlink_metadata;
use super::namespace_mutation::delete_open_entry;
use crate::local::LocalRelativePath;
use crate::local::internal::rooted_directory_reader::RootedDirectoryReader;

/// Byte capacity used for each native directory-enumeration request.
const DIRECTORY_READ_BUFFER_SIZE: usize = 64 * 1024;
/// Reparse-tag bit identifying name-surrogate entries.
const IO_REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;

impl RootedDirectoryReader {
    /// Creates a lazy enumerator for an already-opened directory handle.
    fn new(directory: File) -> Self {
        Self {
            directory,
            buffer: vec![0_usize; DIRECTORY_READ_BUFFER_SIZE.div_ceil(size_of::<usize>())],
            used: 0,
            offset: 0,
            restart: true,
            exhausted: false,
        }
    }

    /// Reads the next child without following a final reparse point.
    ///
    /// Returns `Ok(None)` after all native records are consumed, and returns an
    /// I/O error when native enumeration or child inspection fails.
    pub(crate) fn next_entry(&mut self) -> Result<Option<(OsString, File)>> {
        loop {
            if self.offset >= self.used {
                self.read_next_buffer()?;
                if self.exhausted {
                    return Ok(None);
                }
            }
            let (name, next_offset) = self.current_name()?;
            self.offset = next_offset;
            if name == OsStr::new(".") || name == OsStr::new("..") {
                continue;
            }
            let child = nt_open_at(&self.directory, &name, FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
            return Ok(Some((name, child)));
        }
    }

    /// Requests the next batch of native directory records.
    fn read_next_buffer(&mut self) -> Result<()> {
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: `buffer` and `status_block` are writable for this synchronous
        // request. All optional callback and filter pointers are null.
        let status = unsafe {
            NtQueryDirectoryFile(
                self.directory.as_raw_handle(),
                null_mut(),
                None,
                null(),
                &raw mut status_block,
                self.buffer.as_mut_ptr().cast(),
                DIRECTORY_READ_BUFFER_SIZE as u32,
                FileDirectoryInformation,
                false,
                null(),
                self.restart,
            )
        };
        if status == STATUS_NO_MORE_FILES {
            self.exhausted = true;
            self.used = 0;
            self.offset = 0;
            return Ok(());
        }
        nt_result(status)?;
        self.restart = false;
        self.used = status_block.Information.min(DIRECTORY_READ_BUFFER_SIZE);
        self.offset = 0;
        if self.used == 0 {
            return Err(Error::other("NtQueryDirectoryFile returned an empty record batch"));
        }
        Ok(())
    }

    /// Parses the current native directory record and advances its byte offset.
    fn current_name(&self) -> Result<(OsString, usize)> {
        let name_offset = std::mem::offset_of!(FILE_DIRECTORY_INFORMATION, FileName);
        let remaining = self
            .used
            .checked_sub(self.offset)
            .ok_or_else(|| Error::other("directory record offset exceeded the native result"))?;
        if remaining < name_offset {
            return Err(Error::other("truncated directory record header"));
        }
        // SAFETY: the bounds check above ensures the fixed record header lies
        // inside the valid native result buffer.
        let information = unsafe {
            &*self
                .buffer
                .as_ptr()
                .cast::<u8>()
                .add(self.offset)
                .cast::<FILE_DIRECTORY_INFORMATION>()
        };
        let name_bytes = information.FileNameLength as usize;
        let name_size = name_bytes
            .checked_div(size_of::<u16>())
            .filter(|_| name_bytes.is_multiple_of(size_of::<u16>()))
            .ok_or_else(|| Error::other("directory record name has an invalid length"))?;
        let name_end = name_offset
            .checked_add(name_bytes)
            .ok_or_else(|| Error::other("directory record name length overflowed"))?;
        if name_end > remaining {
            return Err(Error::other("truncated directory record name"));
        }
        // SAFETY: `name_end` was verified within the current native record.
        let name = unsafe { OsString::from_wide(std::slice::from_raw_parts(information.FileName.as_ptr(), name_size)) };
        let next_offset = if information.NextEntryOffset == 0 {
            self.used
        } else {
            self.offset
                .checked_add(information.NextEntryOffset as usize)
                .filter(|next| *next > self.offset && *next <= self.used)
                .ok_or_else(|| Error::other("directory record offset overflowed"))?
        };
        Ok((name, next_offset))
    }
}

/// Opens a lazy reader for immediate children of the opened root.
///
/// Returns an I/O error when the root handle cannot be duplicated.
pub(crate) fn open_root_directory_reader(root: &File, _diagnostic_root: &Path) -> Result<RootedDirectoryReader> {
    root.try_clone().map(RootedDirectoryReader::new)
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

/// Opens a lazy reader for immediate children of a rooted descendant.
///
/// Returns an I/O error when secure traversal or directory opening fails.
pub(crate) fn open_rooted_directory_reader(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<RootedDirectoryReader> {
    let directory = open_entry(
        root,
        path,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE,
    )?;
    verify_real_directory(&directory)?;
    Ok(RootedDirectoryReader::new(directory))
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
            Err(source_error) if !recursive && !final_component && source_error.kind() == ErrorKind::NotFound => {
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
        let entry = read_rooted_symlink_metadata(root, diagnostic_root, &current)?;
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
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn delete_rooted_entry(root: &File, path: &LocalRelativePath) -> Result<()> {
    let entry = open_entry_no_follow(root, path, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
    delete_open_entry(&entry)
}

/// Enumerates one already opened directory with `NtQueryDirectoryFile`.
fn read_directory_handle(directory: &File, _diagnostic_root: &Path) -> Result<Vec<(OsString, File)>> {
    let mut entries = Vec::new();
    let mut reader = RootedDirectoryReader::new(directory.try_clone()?);
    while let Some(entry) = reader.next_entry()? {
        entries.push(entry);
    }
    entries.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
    Ok(entries)
}

/// Rejects name-surrogate reparse points for an opened handle.
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

/// Verifies an opened handle is a real directory rather than a reparse point.
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
