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
    pub(crate) fn from_file(file: File) -> io::Result<Self> {
        let metadata = LocalFileMetadata::from_native(&file.metadata()?);
        Ok(Self { file, metadata })
    }

    /// Returns the underlying native file handle.
    #[inline]
    pub const fn as_file(&self) -> &File {
        &self.file
    }

    /// Returns metadata captured from this reader's retained handle.
    #[inline]
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
        #[cfg(any(windows, feature = "internal-test-support"))]
        {
            let mut total = 0;
            for buffer in buffers {
                #[cfg(feature = "internal-test-support")]
                let result = if total > 0 {
                    if crate::local::test_support_enabled("local-file-reader-vectored-read-after-first") {
                        Err(io::Error::other("injected vectored read failure"))
                    } else {
                        self.read(buffer)
                    }
                } else {
                    self.read(buffer)
                };
                #[cfg(not(feature = "internal-test-support"))]
                let result = self.read(buffer);
                let count = match result {
                    Ok(count) => count,
                    Err(_) if total > 0 => return Ok(total),
                    Err(error) => return Err(error),
                };
                total += count;
                if count < buffer.len() {
                    break;
                }
            }
            Ok(total)
        }
        #[cfg(all(not(windows), not(feature = "internal-test-support")))]
        self.as_file().read_vectored(buffers)
    }
}

impl Seek for LocalFileReader {
    /// Moves the native file cursor and returns its new byte offset.
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
