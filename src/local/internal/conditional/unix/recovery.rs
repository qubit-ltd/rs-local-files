// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Identity checked, read-only recovery access for Unix namespace entries.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use super::RecoveryAccess;
use crate::LocalFileReader;
use crate::LocalResult;

#[derive(Debug)]
pub(super) struct EntryAccess {
    parent: File,
    name: CString,
    device: u64,
    inode: u64,
    diagnostic: Option<PathBuf>,
}

impl EntryAccess {
    pub(super) fn new(
        parent: File,
        name: CString,
        device: u64,
        inode: u64,
        diagnostic: Option<PathBuf>,
    ) -> Self {
        Self {
            parent,
            name,
            device,
            inode,
            diagnostic,
        }
    }
}

impl RecoveryAccess for EntryAccess {
    fn open_reader(&self) -> LocalResult<LocalFileReader> {
        // SAFETY: the parent descriptor and single-component C string remain
        // live for openat; O_NOFOLLOW prevents substituting a symlink target.
        let descriptor = unsafe {
            libc::openat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if descriptor == -1 {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: openat returned a new descriptor uniquely owned here.
        let file = unsafe { File::from_raw_fd(descriptor) };
        let metadata = file.metadata().map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                error,
            )
        })?;
        if !metadata.is_file() || metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                io::Error::new(io::ErrorKind::NotFound, "recovery entry identity changed"),
            ));
        }
        LocalFileReader::from_file(file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                error,
            )
        })
    }
}

pub(super) fn name(entry: &super::super::ConditionalEntry) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    let name = entry.namespace_path().file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "entry path has no final component",
        )
    })?;
    CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "entry name contains NUL"))
}
