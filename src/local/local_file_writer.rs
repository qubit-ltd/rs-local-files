// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local file writer wrapper.

use std::fs::File;
use std::io::{
    BufWriter,
    Result,
    Seek,
    SeekFrom,
    Write,
};

use crate::FileBuffering;

/// Writer returned by local file write APIs.
///
/// Additional writer representations may be added in future releases. Match
/// with a wildcard arm when inspecting the representation directly.
///
/// ```compile_fail
/// use qubit_local_files::LocalFileWriter;
///
/// fn consume(writer: LocalFileWriter) {
///     match writer {
///         LocalFileWriter::Unbuffered(_) => {}
///         LocalFileWriter::Buffered(_) => {}
///     }
/// }
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub enum LocalFileWriter {
    /// Unbuffered writer backed directly by a [`File`].
    Unbuffered(File),
    /// Buffered writer backed by a [`BufWriter<File>`].
    Buffered(BufWriter<File>),
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
    #[inline]
    pub(crate) fn from_file(file: File, buffering: FileBuffering) -> Self {
        match buffering {
            FileBuffering::Unbuffered => Self::Unbuffered(file),
            FileBuffering::Buffered { capacity: None } => {
                Self::Buffered(BufWriter::new(file))
            }
            FileBuffering::Buffered {
                capacity: Some(capacity),
            } => Self::Buffered(BufWriter::with_capacity(capacity.get(), file)),
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
    /// `true` when the writer is backed by [`BufWriter`].
    #[inline(always)]
    pub const fn is_buffered(&self) -> bool {
        matches!(self, Self::Buffered(_))
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
    pub fn sync_all(&mut self) -> Result<()> {
        self.flush()?;
        match self {
            Self::Unbuffered(file) => file.sync_all(),
            Self::Buffered(writer) => writer.get_ref().sync_all(),
        }
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
    pub fn sync_data(&mut self) -> Result<()> {
        self.flush()?;
        match self {
            Self::Unbuffered(file) => file.sync_data(),
            Self::Buffered(writer) => writer.get_ref().sync_data(),
        }
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
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        match self {
            Self::Unbuffered(file) => file.write(buf),
            Self::Buffered(writer) => writer.write(buf),
        }
    }

    /// Flushes the wrapped file writer.
    ///
    /// # Errors
    /// Returns the I/O error reported by the wrapped writer.
    #[inline]
    fn flush(&mut self) -> Result<()> {
        match self {
            Self::Unbuffered(file) => file.flush(),
            Self::Buffered(writer) => writer.flush(),
        }
    }
}

impl Seek for LocalFileWriter {
    /// Repositions the wrapped file writer.
    ///
    /// Buffered writers flush pending bytes before seeking, matching
    /// [`BufWriter`] seek semantics. Seeking does not disable append mode:
    /// writers opened with append semantics may still write at the end of the
    /// file according to [`std::fs::OpenOptions`] behavior.
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
    #[inline]
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        match self {
            Self::Unbuffered(file) => file.seek(pos),
            Self::Buffered(writer) => writer.seek(pos),
        }
    }
}
