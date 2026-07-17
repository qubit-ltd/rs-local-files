// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private storage representations for local file writers.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs::File;
use std::io::{
    BufWriter,
    Result,
    Seek,
    SeekFrom,
    Write,
};

use crate::FileBuffering;

/// Owns the private concrete representation of a local file writer.
#[derive(Debug)]
pub(in crate::local) enum LocalFileWriterInner {
    /// Writer backed directly by an unbuffered file handle.
    Unbuffered(File),
    /// Writer backed by a standard-library buffer.
    Buffered(BufWriter<File>),
}

impl LocalFileWriterInner {
    /// Wraps `file` according to `buffering`.
    ///
    /// # Parameters
    ///
    /// * `file` - File handle opened for writing.
    /// * `buffering` - Buffering policy for the private representation.
    ///
    /// # Returns
    ///
    /// A writer representation matching `buffering`.
    #[inline]
    pub(in crate::local) fn from_file(
        file: File,
        buffering: FileBuffering,
    ) -> Self {
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

    /// Reports whether this representation uses a userspace buffer.
    ///
    /// # Returns
    ///
    /// `true` for the buffered representation; otherwise, `false`.
    #[inline(always)]
    pub(in crate::local) const fn is_buffered(&self) -> bool {
        matches!(self, Self::Buffered(_))
    }

    /// Synchronizes file contents and metadata after flushing userspace data.
    ///
    /// # Errors
    ///
    /// Returns the I/O error reported while flushing or synchronizing.
    #[inline]
    pub(in crate::local) fn sync_all(&mut self) -> Result<()> {
        self.flush()?;
        match self {
            Self::Unbuffered(file) => file.sync_all(),
            Self::Buffered(writer) => writer.get_ref().sync_all(),
        }
    }

    /// Synchronizes file contents after flushing userspace data.
    ///
    /// # Errors
    ///
    /// Returns the I/O error reported while flushing or synchronizing.
    #[inline]
    pub(in crate::local) fn sync_data(&mut self) -> Result<()> {
        self.flush()?;
        match self {
            Self::Unbuffered(file) => file.sync_data(),
            Self::Buffered(writer) => writer.get_ref().sync_data(),
        }
    }
}

impl Write for LocalFileWriterInner {
    /// Writes through the active private representation.
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        match self {
            Self::Unbuffered(file) => file.write(buffer),
            Self::Buffered(writer) => writer.write(buffer),
        }
    }

    /// Flushes through the active private representation.
    #[inline(always)]
    fn flush(&mut self) -> Result<()> {
        match self {
            Self::Unbuffered(file) => file.flush(),
            Self::Buffered(writer) => writer.flush(),
        }
    }
}

impl Seek for LocalFileWriterInner {
    /// Seeks through the active private representation.
    #[inline(always)]
    fn seek(&mut self, position: SeekFrom) -> Result<u64> {
        match self {
            Self::Unbuffered(file) => file.seek(position),
            Self::Buffered(writer) => writer.seek(position),
        }
    }
}
