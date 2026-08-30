// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Lazy Unix directory enumeration.

use std::ffi::CString;
use std::ffi::OsString;
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use rustix::fs::Dir;

use super::EntryIdentity;
use super::PlatformDirectoryEntry;
use super::namespace_handle::kind_from_mode;
use super::namespace_handle::stat_child;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Lazily enumerates one already-opened Unix directory.
#[derive(Debug)]
pub(crate) struct DirectoryCursor {
    /// Descriptor used to inspect child entries without following them.
    directory: File,
    /// Native directory stream used for incremental enumeration.
    stream: Dir,
    /// Relative path retained for structured error context.
    path: PathBuf,
}

impl DirectoryCursor {
    /// Opens a cursor over an already-authorized directory descriptor.
    ///
    /// # Parameters
    ///
    /// - `directory`: Open directory descriptor.
    /// - `path`: Authority-relative directory path used for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a list error when the descriptor cannot be duplicated into a
    /// native directory stream.
    pub(super) fn open(directory: File, path: PathBuf) -> LocalResult<Self> {
        let stream = Dir::read_from(&directory).map_err(|error| {
            crate::LocalFileError::from_io(LocalFileOperation::List, Some(path.clone()), None, error.into())
        })?;
        Ok(Self {
            directory,
            stream,
            path,
        })
    }

    /// Reads the next immediate child without following its final component.
    ///
    /// # Returns
    ///
    /// Returns `Some` for the next child and `None` after enumeration is
    /// exhausted.
    ///
    /// # Errors
    ///
    /// Returns a list error when enumeration or no-follow child inspection
    /// fails.
    pub(crate) fn next_entry(&mut self) -> LocalResult<Option<PlatformDirectoryEntry>> {
        loop {
            let entry = match self.stream.next() {
                Some(result) => result.map_err(|error| {
                    crate::LocalFileError::from_io(
                        LocalFileOperation::List,
                        Some(self.path.clone()),
                        None,
                        error.into(),
                    )
                })?,
                None => return Ok(None),
            };
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = OsString::from_vec(bytes.to_vec());
            let c_name = CString::new(name.as_bytes()).expect("native directory entries never contain NUL");
            let status = stat_child(
                &self.directory,
                &c_name,
                LocalFileOperation::List,
                &self.path.join(&name),
            )?;
            return Ok(Some(PlatformDirectoryEntry::new(
                name,
                kind_from_mode(status.st_mode),
                EntryIdentity::from_stat(&status),
            )));
        }
    }
}
