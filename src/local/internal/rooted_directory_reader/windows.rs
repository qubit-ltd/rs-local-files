// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative lazy directory enumeration.
// qubit-style: allow source-test-pair

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io::Error;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
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

use super::RootedDirectoryReader;
use crate::local::internal::windows_rooted::native::nt_open_at;
use crate::local::internal::windows_rooted::native::nt_result;

/// Byte capacity used for each native directory-enumeration request.
const DIRECTORY_READ_BUFFER_SIZE: usize = 64 * 1024;

impl RootedDirectoryReader {
    /// Creates a lazy enumerator for an already-opened directory handle.
    pub(in crate::local::internal) fn new(directory: File) -> Self {
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
    /// Updates buffer bounds and marks exhaustion on `STATUS_NO_MORE_FILES`.
    /// Propagates native errors and rejects an unexpectedly empty result batch.
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

    /// Parses the current record and returns its name and next byte offset.
    /// Does not mutate iterator state. Rejects truncated headers/names, odd
    /// UTF-16 byte lengths, and invalid or overflowing next-record offsets.
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
