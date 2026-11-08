// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix descriptor-relative lazy directory enumeration.

use std::ffi::CString;
use std::ffi::OsString;
use std::fs::File;
use std::io::Result;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

use rustix::fs::Dir;

use super::RootedDirectoryReader;
use crate::local::internal::RootedDirectoryReadError;
use crate::local::internal::rooted_directory_entry::RootedDirectoryEntry;
use crate::local::internal::rooted_directory_entry::stat_child;

impl RootedDirectoryReader {
    /// Opens a reader over an already-authorized directory descriptor.
    ///
    /// # Parameters
    ///
    /// * `directory` is the authorized descriptor retained for enumeration and
    ///   child inspection.
    /// * `diagnostic_path` is retained only to enrich native I/O errors.
    ///
    /// Returns an I/O error when the descriptor cannot be duplicated for
    /// enumeration.
    pub(in crate::local::internal) fn open(directory: File, diagnostic_path: &Path) -> Result<Self> {
        let stream = Dir::read_from(&directory)?;
        Ok(Self {
            directory,
            stream,
            diagnostic_path: diagnostic_path.to_path_buf(),
        })
    }

    /// Reads the next child without following its final symbolic link.
    ///
    /// Returns `Ok(None)` after the directory is exhausted, and returns an I/O
    /// error when enumeration or no-follow metadata inspection fails.
    pub(crate) fn next_entry(&mut self) -> Result<Option<RootedDirectoryEntry>> {
        self.next_entry_with_context()
            .map_err(RootedDirectoryReadError::into_io_error)
    }

    /// Reads the next child while retaining whether a failure followed cursor
    /// advancement.
    pub(crate) fn next_entry_with_context(
        &mut self,
    ) -> std::result::Result<Option<RootedDirectoryEntry>, RootedDirectoryReadError> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("rooted-stream-read") {
            return Err(RootedDirectoryReadError::stream(crate::local::test_fault_error()));
        }
        loop {
            let entry = match self.stream.next() {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    return Err(RootedDirectoryReadError::stream(std::io::Error::from(error)));
                }
                None => return Ok(None),
            };
            let name = entry.file_name();
            let name = name.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            let name = OsString::from_vec(name.to_vec());
            let c_name = CString::new(name.as_bytes()).expect("directory entry names never contain NUL");
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support_on_nth("rooted-child-inspection-first", 1)
                || crate::local::take_test_support_on_nth("rooted-child-inspection", 2)
            {
                return Err(RootedDirectoryReadError::entry(name, crate::local::test_fault_error()));
            }
            let status = match stat_child(&self.directory, &c_name, &self.diagnostic_path) {
                Ok(status) => status,
                Err(error) => return Err(RootedDirectoryReadError::entry(name, error)),
            };
            return Ok(Some((name, status)));
        }
    }
}
