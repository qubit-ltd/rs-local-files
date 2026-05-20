/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Local file reader wrapper.

use std::fs::File;
use std::io::{
    BufReader,
    Error,
    ErrorKind,
    Read,
    Result,
};

use crate::FileBuffering;

/// Reader returned by local file read APIs.
#[derive(Debug)]
pub enum LocalFileReader {
    /// Unbuffered reader backed directly by a [`File`].
    Unbuffered(File),
    /// Buffered reader backed by a [`BufReader<File>`].
    Buffered(BufReader<File>),
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
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidInput`] when a buffered reader requests a
    /// zero-byte capacity.
    pub(crate) fn from_file(file: File, buffering: FileBuffering) -> Result<Self> {
        match buffering {
            FileBuffering::Unbuffered => Ok(Self::Unbuffered(file)),
            FileBuffering::Buffered { capacity: None } => Ok(Self::Buffered(BufReader::new(file))),
            FileBuffering::Buffered {
                capacity: Some(capacity),
            } => {
                validate_buffer_capacity(capacity)?;
                Ok(Self::Buffered(BufReader::with_capacity(capacity, file)))
            }
        }
    }

    /// Returns whether this reader is buffered.
    ///
    /// # Returns
    /// `true` when the reader is backed by [`BufReader`].
    #[inline]
    pub const fn is_buffered(&self) -> bool {
        matches!(self, Self::Buffered(_))
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
        match self {
            Self::Unbuffered(file) => file.read(buf),
            Self::Buffered(reader) => reader.read(buf),
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
