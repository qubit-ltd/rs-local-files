// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local file reader wrapper.

use std::fs::File;
use std::io::{
    IoSliceMut,
    Read,
    Result,
    Seek,
    SeekFrom,
};

use crate::FileBuffering;

use super::internal::LocalFileReaderInner;

/// Reader returned by local file read APIs.
///
/// Its concrete buffering representation is intentionally private so callers
/// depend only on the stable [`Read`] and [`Seek`] behavior.
///
/// ```compile_fail
/// use qubit_local_files::LocalFileReader;
///
/// let _constructor: fn(std::fs::File) -> LocalFileReader =
///     LocalFileReader::Unbuffered;
/// ```
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::{FileReadOptions, LocalFiles};
///
/// LocalFiles::open_reader("input.txt", FileReadOptions::default())
///     .expect("reader should open");
/// ```
#[must_use = "discarding the reader immediately closes the opened file"]
#[derive(Debug)]
pub struct LocalFileReader {
    /// Private concrete reader representation.
    inner: LocalFileReaderInner,
}

impl LocalFileReader {
    /// Wraps a file handle according to a buffering policy.
    ///
    /// # Parameters
    /// - `file`: File handle opened for reading.
    /// - `buffering`: Buffering policy for the returned reader.
    ///
    /// # Returns
    /// A local file reader matching `buffering`.
    #[inline(always)]
    pub(crate) fn from_file(file: File, buffering: FileBuffering) -> Self {
        Self {
            inner: LocalFileReaderInner::from_file(file, buffering),
        }
    }

    /// Returns whether this reader is buffered.
    ///
    /// # Returns
    /// `true` when the reader uses a userspace buffer.
    #[must_use]
    #[inline(always)]
    pub const fn is_buffered(&self) -> bool {
        self.inner.is_buffered()
    }
}

impl Read for LocalFileReader {
    /// Reads bytes from the wrapped file reader.
    ///
    /// # Parameters
    /// - `buf`: Destination byte buffer.
    ///
    /// # Returns
    /// Number of bytes read.
    ///
    /// # Errors
    /// Returns the I/O error reported by the wrapped reader.
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.inner.read(buf)
    }

    /// Reads bytes into multiple destination buffers.
    ///
    /// # Parameters
    /// - `buffers`: Destination buffers filled by the wrapped reader.
    ///
    /// # Returns
    /// Total number of bytes read across the supplied buffers.
    ///
    /// # Errors
    /// Returns the I/O error reported by the wrapped reader.
    #[inline(always)]
    fn read_vectored(
        &mut self,
        buffers: &mut [IoSliceMut<'_>],
    ) -> Result<usize> {
        self.inner.read_vectored(buffers)
    }
}

impl Seek for LocalFileReader {
    /// Repositions the wrapped file reader.
    ///
    /// # Parameters
    /// - `pos`: Target seek position.
    ///
    /// # Returns
    /// New absolute stream position.
    ///
    /// # Errors
    /// Returns the I/O error reported by the wrapped reader.
    #[inline(always)]
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.inner.seek(pos)
    }
}
