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

use crate::platform::OpenedFile;

/// Owned synchronous reader for an opened native regular file.
#[derive(Debug)]
pub struct LocalFileReader {
    /// Native file handle and, for authority-opened readers, its observations.
    file: LocalFileReaderInner,
}

#[derive(Debug)]
enum LocalFileReaderInner {
    Plain(File),
    Opened(OpenedFile),
}

impl LocalFileReader {
    /// Wraps an already validated native regular-file handle.
    ///
    /// # Parameters
    ///
    /// - `file`: Open native file handle.
    pub(crate) const fn new(file: File) -> Self {
        Self {
            file: LocalFileReaderInner::Plain(file),
        }
    }

    /// Wraps a descriptor together with observations captured from that same
    /// descriptor.
    pub(crate) fn from_opened(file: OpenedFile) -> Self {
        Self {
            file: LocalFileReaderInner::Opened(file),
        }
    }

    /// Returns the underlying native file handle.
    #[must_use]
    pub const fn as_file(&self) -> &File {
        match &self.file {
            LocalFileReaderInner::Plain(file) => file,
            LocalFileReaderInner::Opened(file) => file.file(),
        }
    }

    /// Returns metadata captured from an authority-opened descriptor.
    pub fn metadata(&self) -> Option<&crate::LocalFileMetadata> {
        match &self.file {
            LocalFileReaderInner::Plain(_) => None,
            LocalFileReaderInner::Opened(file) => Some(file.metadata()),
        }
    }
}

impl Read for LocalFileReader {
    /// Reads bytes from the native file at its current offset.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.file {
            LocalFileReaderInner::Plain(file) => file.read(buffer),
            LocalFileReaderInner::Opened(file) => file.read(buffer),
        }
    }

    /// Reads bytes into multiple buffers from the current offset.
    fn read_vectored(
        &mut self,
        buffers: &mut [IoSliceMut<'_>],
    ) -> io::Result<usize> {
        #[cfg(any(windows, feature = "internal-test-support"))]
        {
            let mut total = 0;
            for buffer in buffers {
                #[cfg(feature = "internal-test-support")]
                let result = if total > 0 {
                    if crate::local::test_support_enabled(
                        "local-file-reader-vectored-read-after-first",
                    ) {
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
        match &mut self.file {
            LocalFileReaderInner::Plain(file) => file.seek(position),
            LocalFileReaderInner::Opened(file) => {
                file.file_mut().seek(position)
            }
        }
    }
}
