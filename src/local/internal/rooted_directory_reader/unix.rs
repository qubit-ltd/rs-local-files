// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix descriptor-relative lazy directory enumeration.
// qubit-style: allow source-test-pair

use std::ffi::CString;
use std::ffi::OsString;
use std::fs::File;
use std::io::Result;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

use rustix::fs::Dir;

use super::RootedDirectoryReader;
use crate::local::internal::rooted_directory_entry::RootedDirectoryEntry;
use crate::local::internal::rooted_directory_entry::stat_child;

impl RootedDirectoryReader {
    /// Opens a reader over an already-authorized directory descriptor.
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
        loop {
            let entry = match self.stream.next() {
                Some(entry) => entry?,
                None => return Ok(None),
            };
            let name = entry.file_name();
            let name = name.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            let name = OsString::from_vec(name.to_vec());
            let c_name = CString::new(name.as_bytes()).expect("directory entry names never contain NUL");
            let status = stat_child(&self.directory, &c_name, &self.diagnostic_path)?;
            return Ok(Some((name, status)));
        }
    }
}
