// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Local file reader wrapper.

use std::fs::File;
use std::io::{
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
    #[inline]
    pub(crate) fn from_file(file: File, buffering: FileBuffering) -> Self {
        Self {
            inner: LocalFileReaderInner::from_file(file, buffering),
        }
    }

    /// Returns whether this reader is buffered.
    ///
    /// # Returns
    /// `true` when the reader uses a userspace buffer.
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
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.inner.read(buf)
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
    #[inline]
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.inner.seek(pos)
    }
}
