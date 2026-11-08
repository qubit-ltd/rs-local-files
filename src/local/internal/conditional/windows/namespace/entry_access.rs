// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Identity-bound permission for reopening a conditional namespace entry.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::io::ErrorKind;
use std::path::PathBuf;

use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::super::super::RecoveryAccess;
use super::ENTRY_ACCESS;
use super::file_identity;
use super::verify_regular_no_reparse;
use crate::LocalFileReader;
use crate::LocalResult;
use crate::local::internal::windows_rooted::open_conditional_entry_at;

/// Retains the authority and identity needed to reopen one namespace entry.
#[derive(Debug)]
pub(super) struct EntryAccess {
    /// Directory handle used for relative opens.
    pub(super) parent: File,
    /// Entry leaf name reopened beneath `parent`.
    pub(super) name: OsString,
    /// Optional path used only when constructing user-facing errors.
    pub(super) diagnostic_path: Option<PathBuf>,
    /// Identity every reopened entry must retain.
    pub(super) identity: super::FileIdentity,
    /// Original file handle retained to preserve access authority when needed.
    pub(super) _retained: Option<File>,
}

impl RecoveryAccess for EntryAccess {
    /// Reopens the entry only while its identity and regular-file type remain
    /// valid.
    ///
    /// # Returns
    ///
    /// A reader for the same file identity retained by this permission.
    ///
    /// # Errors
    ///
    /// Returns a local file error if reopening, identity verification, type
    /// validation, or reader construction fails.
    fn open_reader(&self) -> LocalResult<LocalFileReader> {
        let file = open_conditional_entry_at(
            &self.parent,
            &self.name,
            ENTRY_ACCESS,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
        )
        .map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })?;
        let observed = file_identity(&file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })?;
        verify_regular_no_reparse(&file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })?;
        if observed != self.identity {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                io::Error::new(ErrorKind::InvalidData, "conditional recovery entry identity changed"),
            ));
        }
        LocalFileReader::from_file(file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })
    }

    /// Flushes the same verified file object represented by this permission.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reopening, type validation, identity
    /// verification, or synchronization fails.
    fn sync_file(&self) -> io::Result<()> {
        let file = open_conditional_entry_at(
            &self.parent,
            &self.name,
            GENERIC_READ | GENERIC_WRITE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
        )?;
        verify_regular_no_reparse(&file)?;
        if file_identity(&file)? != self.identity {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "conditional entry identity changed before file synchronization",
            ));
        }
        file.sync_all()
    }
}
