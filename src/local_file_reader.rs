// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by host and rooted reader integration tests.

use std::{
    fs::File,
    io::{
        self,
        IoSliceMut,
        Read,
        Seek,
        SeekFrom,
    },
};

/// Owned synchronous reader for an opened native regular file.
#[derive(Debug)]
pub struct LocalFileReader {
    /// Native file handle.
    file: File,
}

impl LocalFileReader {
    /// Wraps an already validated native regular-file handle.
    ///
    /// # Parameters
    ///
    /// - `file`: Open native file handle.
    pub(crate) const fn new(file: File) -> Self {
        Self { file }
    }

    /// Returns the underlying native file handle.
    #[must_use]
    pub const fn as_file(&self) -> &File {
        &self.file
    }
}

impl Read for LocalFileReader {
    /// Reads bytes from the native file at its current offset.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }

    /// Reads bytes into multiple buffers from the current offset.
    fn read_vectored(
        &mut self,
        buffers: &mut [IoSliceMut<'_>],
    ) -> io::Result<usize> {
        #[cfg(windows)]
        {
            let mut total = 0;
            for buffer in buffers {
                let count = self.file.read(buffer)?;
                total += count;
                if count < buffer.len() {
                    break;
                }
            }
            Ok(total)
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
