// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Lazy Windows directory enumeration.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_INFORMATION;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FileDirectoryInformation;
use windows_sys::Wdk::Storage::FileSystem::NtQueryDirectoryFile;
use windows_sys::Win32::Foundation::STATUS_NO_MORE_FILES;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::EntryIdentity;
use super::PlatformDirectoryEntry;
use super::namespace_handle::io_error;
use super::namespace_handle::nt_open_at;
use super::namespace_handle::nt_result;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Byte capacity used for each native enumeration request.
const DIRECTORY_READ_BUFFER_SIZE: usize = 64 * 1024;

/// Lazily enumerates one already-opened Windows directory.
#[derive(Debug)]
pub(crate) struct DirectoryCursor {
    /// Directory handle retained for enumeration and child inspection.
    directory: File,
    /// Aligned storage for native directory records.
    buffer: Vec<usize>,
    /// Number of valid bytes in `buffer`.
    used: usize,
    /// Offset of the next record.
    offset: usize,
    /// Whether the next request restarts enumeration.
    restart: bool,
    /// Whether native enumeration is exhausted.
    exhausted: bool,
    /// Relative path retained for error context.
    path: PathBuf,
}

impl DirectoryCursor {
    /// Creates a cursor for an already-verified directory handle.
    #[must_use]
    pub(super) fn new(directory: File, path: PathBuf) -> Self {
        Self {
            directory,
            buffer: vec![0_usize; DIRECTORY_READ_BUFFER_SIZE.div_ceil(size_of::<usize>())],
            used: 0,
            offset: 0,
            restart: true,
            exhausted: false,
            path,
        }
    }

    /// Reads the next immediate child without following its final reparse
    /// point.
    ///
    /// # Returns
    ///
    /// Returns `Some` for the next child and `None` after enumeration is
    /// exhausted.
    ///
    /// # Errors
    ///
    /// Returns a list error when native enumeration, record validation, child
    /// opening, metadata, or identity capture fails.
    pub(crate) fn next_entry(&mut self) -> LocalResult<Option<PlatformDirectoryEntry>> {
        loop {
            if self.offset >= self.used {
                self.read_next_buffer()
                    .map_err(|error| io_error(LocalFileOperation::List, &self.path, None, error))?;
                if self.exhausted {
                    return Ok(None);
                }
            }
            let (name, next_offset) = self
                .current_name()
                .map_err(|error| io_error(LocalFileOperation::List, &self.path, None, error))?;
            self.offset = next_offset;
            if name == OsStr::new(".") || name == OsStr::new("..") {
                continue;
            }
            let child = nt_open_at(&self.directory, &name, FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)
                .map_err(|error| io_error(LocalFileOperation::List, &self.path.join(&name), None, error))?;
            let metadata = child
                .metadata()
                .map_err(|error| io_error(LocalFileOperation::List, &self.path.join(&name), None, error))?;
            let identity = EntryIdentity::from_file(&child)
                .map_err(|error| io_error(LocalFileOperation::List, &self.path.join(&name), None, error))?;
            return Ok(Some(PlatformDirectoryEntry::new(
                name,
                LocalFileMetadata::from_native(&metadata).kind(),
                identity,
            )));
        }
    }

    /// Requests the next native directory-record batch.
    fn read_next_buffer(&mut self) -> io::Result<()> {
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: the buffer and status block are writable for this synchronous
        // request, and all optional callback/filter pointers are null.
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
            return Err(io::Error::other("NtQueryDirectoryFile returned an empty record batch"));
        }
        Ok(())
    }

    /// Parses the current directory record and returns its next byte offset.
    fn current_name(&self) -> io::Result<(OsString, usize)> {
        let name_offset = std::mem::offset_of!(FILE_DIRECTORY_INFORMATION, FileName);
        let remaining = self
            .used
            .checked_sub(self.offset)
            .ok_or_else(|| io::Error::other("directory record offset exceeded result size"))?;
        if remaining < name_offset {
            return Err(io::Error::other("truncated directory record header"));
        }
        // SAFETY: the fixed header was bounds-checked inside the valid buffer.
        let information = unsafe {
            &*self
                .buffer
                .as_ptr()
                .cast::<u8>()
                .add(self.offset)
                .cast::<FILE_DIRECTORY_INFORMATION>()
        };
        let name_bytes = information.FileNameLength as usize;
        let name_units = name_bytes
            .checked_div(size_of::<u16>())
            .filter(|_| name_bytes.is_multiple_of(size_of::<u16>()))
            .ok_or_else(|| io::Error::other("directory record name has invalid length"))?;
        let name_end = name_offset
            .checked_add(name_bytes)
            .ok_or_else(|| io::Error::other("directory record name length overflowed"))?;
        if name_end > remaining {
            return Err(io::Error::other("truncated directory record name"));
        }
        // SAFETY: `name_end` was verified inside the current record.
        let name =
            unsafe { OsString::from_wide(std::slice::from_raw_parts(information.FileName.as_ptr(), name_units)) };
        let next_offset = if information.NextEntryOffset == 0 {
            self.used
        } else {
            self.offset
                .checked_add(information.NextEntryOffset as usize)
                .filter(|next| *next > self.offset && *next <= self.used)
                .ok_or_else(|| io::Error::other("directory record offset overflowed"))?
        };
        Ok((name, next_offset))
    }
}
