// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local file writer wrapper.

use std::fs::File;
use std::io::{
    Result,
    Seek,
    SeekFrom,
    Write,
};

use crate::FileBuffering;

use super::internal::LocalFileWriterInner;

/// Writer returned by local file write APIs.
///
/// Its concrete buffering representation is intentionally private so callers
/// depend only on the stable [`Write`] and [`Seek`] behavior.
///
/// ```compile_fail
/// use qubit_local_files::LocalFileWriter;
///
/// let _constructor: fn(std::io::BufWriter<std::fs::File>) -> LocalFileWriter =
///     LocalFileWriter::Buffered;
/// ```
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::{FileWriteOptions, LocalFiles};
///
/// LocalFiles::open_writer("output.txt", FileWriteOptions::default())
///     .expect("writer should open");
/// ```
#[must_use = "discarding the writer may hide buffered write or flush failures"]
#[derive(Debug)]
pub struct LocalFileWriter {
    /// Private concrete writer representation.
    inner: LocalFileWriterInner,
}

impl LocalFileWriter {
    /// Wraps a file handle according to a buffering policy.
    ///
    /// # Parameters
    /// - `file`: File handle opened for writing.
    /// - `buffering`: Buffering policy for the returned writer.
    ///
    /// # Returns
    /// A local file writer matching `buffering`.
    #[inline(always)]
    pub(crate) fn from_file(file: File, buffering: FileBuffering) -> Self {
        Self {
            inner: LocalFileWriterInner::from_file(file, buffering),
        }
    }

    /// Flushes buffered data and closes the writer.
    ///
    /// Closing a standard-library file handle is performed by dropping it. This
    /// method reports flush errors, then consumes the writer so it cannot be
    /// used again by the caller.
    ///
    /// # Errors
    /// Returns the I/O error reported while flushing the wrapped writer.
    #[inline(always)]
    pub fn close(mut self) -> Result<()> {
        self.flush()
    }

    /// Returns whether this writer is buffered.
    ///
    /// # Returns
    /// `true` when the writer uses a userspace buffer.
    #[must_use]
    #[inline(always)]
    pub const fn is_buffered(&self) -> bool {
        self.inner.is_buffered()
    }

    /// Flushes buffered bytes and synchronizes file contents and metadata to
    /// storage.
    ///
    /// This method delegates to [`File::sync_all`] after flushing any
    /// standard-library buffer owned by this writer. It does not close the
    /// writer, so callers may continue writing after it succeeds.
    ///
    /// # Errors
    /// Returns the I/O error reported while flushing or synchronizing the
    /// wrapped file.
    #[inline(always)]
    pub fn sync_all(&mut self) -> Result<()> {
        self.inner.sync_all()
    }

    /// Flushes buffered bytes and synchronizes file contents to storage.
    ///
    /// This method delegates to [`File::sync_data`] after flushing any
    /// standard-library buffer owned by this writer. Metadata synchronization
    /// is platform-dependent and follows [`File::sync_data`] semantics.
    ///
    /// # Errors
    /// Returns the I/O error reported while flushing or synchronizing the
    /// wrapped file.
    #[inline(always)]
    pub fn sync_data(&mut self) -> Result<()> {
        self.inner.sync_data()
    }
}

impl Write for LocalFileWriter {
    /// Writes bytes to the wrapped file writer.
    ///
    /// # Parameters
    /// - `buf`: Source byte buffer.
    ///
    /// # Returns
    /// Number of bytes accepted by the wrapped writer.
    ///
    /// # Errors
    /// Returns the I/O error reported by the wrapped writer.
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.inner.write(buf)
    }

    /// Flushes the wrapped file writer.
    ///
    /// # Errors
    /// Returns the I/O error reported by the wrapped writer.
    #[inline(always)]
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

impl Seek for LocalFileWriter {
    /// Repositions the wrapped file writer.
    ///
    /// Buffered writers flush pending bytes before seeking, matching
    /// [`std::io::BufWriter`] seek semantics. Seeking does not disable append
    /// mode: writers opened with append semantics may still write at the
    /// end of the file according to [`std::fs::OpenOptions`] behavior.
    ///
    /// # Parameters
    /// - `pos`: Target seek position.
    ///
    /// # Returns
    /// New absolute stream position.
    ///
    /// # Errors
    /// Returns the I/O error reported while flushing buffered bytes or seeking
    /// the wrapped file.
    #[inline(always)]
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.inner.seek(pos)
    }
}
