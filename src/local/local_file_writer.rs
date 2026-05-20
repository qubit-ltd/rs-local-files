/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Local file writer wrapper.

use std::fs::File;
use std::io::{
    BufWriter,
    Error,
    ErrorKind,
    Result,
    Write,
};

use crate::FileBuffering;

/// Writer returned by local file write APIs.
#[derive(Debug)]
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
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidInput`] when a buffered writer requests a
    /// zero-byte capacity.
    pub(crate) fn from_file(file: File, buffering: FileBuffering) -> Result<Self> {
        match buffering {
            FileBuffering::Unbuffered => Ok(Self::Unbuffered(file)),
            FileBuffering::Buffered { capacity: None } => Ok(Self::Buffered(BufWriter::new(file))),
            FileBuffering::Buffered {
                capacity: Some(capacity),
            } => {
                validate_buffer_capacity(capacity)?;
                Ok(Self::Buffered(BufWriter::with_capacity(capacity, file)))
            }
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
    #[inline]
    pub fn close(mut self) -> Result<()> {
        self.flush()
    }

    /// Returns whether this writer is buffered.
    ///
    /// # Returns
    /// `true` when the writer is backed by [`BufWriter`].
    #[inline]
    pub const fn is_buffered(&self) -> bool {
        matches!(self, Self::Buffered(_))
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

/// Validates a custom buffer capacity.
///
/// # Parameters
/// - `capacity`: Buffer capacity in bytes.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `capacity` is zero.
fn validate_buffer_capacity(capacity: usize) -> Result<()> {
    if capacity == 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "buffer capacity must be greater than zero",
        ));
    }
    Ok(())
}
