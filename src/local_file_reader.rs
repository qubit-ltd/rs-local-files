// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by host and rooted reader integration tests.

use std::fs::File;
use std::io;
use std::io::IoSliceMut;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use crate::LocalFileMetadata;

/// Owned synchronous reader for an opened native regular file.
///
/// # Examples
///
/// ```no_run
/// use std::io::Read;
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut reader = filesystem.open_reader(Path::new("Cargo.toml"))?;
/// let mut contents = String::new();
/// reader.read_to_string(&mut contents)?;
/// assert!(!contents.is_empty());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct LocalFileReader {
    /// Open native file handle.
    file: File,
    /// Metadata observed from the same handle after it was opened.
    metadata: LocalFileMetadata,
}

impl LocalFileReader {
    /// Wraps an already validated native regular-file handle.
    ///
    /// # Parameters
    ///
    /// - `file`: Open native file handle.
    ///
    /// Takes ownership and captures metadata from the retained handle.
    /// Propagates metadata errors, closing the supplied handle on failure.
    pub(crate) fn from_file(file: File) -> io::Result<Self> {
        let metadata = LocalFileMetadata::from_native(&file.metadata()?);
        Ok(Self { file, metadata })
    }

    /// Returns the underlying native file handle.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn as_file(&self) -> &File {
        &self.file
    }

    /// Returns metadata captured from this reader's retained handle.
    #[must_use = "the retained-handle metadata should be inspected"]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }
}

impl Read for LocalFileReader {
    /// Reads bytes from the native file at its current offset.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }

    /// Reads bytes into multiple buffers from the current offset.
    fn read_vectored(&mut self, buffers: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        #[cfg(windows)]
        {
            crate::read::read_vectored_fallback(&mut self.file, buffers)
        }
        #[cfg(not(windows))]
        self.file.read_vectored(buffers)
    }
}

impl Seek for LocalFileReader {
    /// Moves the native file cursor and returns its new byte offset.
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
