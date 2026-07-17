// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private storage representations for local file readers.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs::File;
use std::io::{
    BufReader,
    Read,
    Result,
    Seek,
    SeekFrom,
};

use crate::FileBuffering;

/// Owns the private concrete representation of a local file reader.
#[derive(Debug)]
pub(in crate::local) enum LocalFileReaderInner {
    /// Reader backed directly by an unbuffered file handle.
    Unbuffered(File),
    /// Reader backed by a standard-library buffer.
    Buffered(BufReader<File>),
}

impl LocalFileReaderInner {
    /// Wraps `file` according to `buffering`.
    ///
    /// # Parameters
    ///
    /// * `file` - File handle opened for reading.
    /// * `buffering` - Buffering policy for the private representation.
    ///
    /// # Returns
    ///
    /// A reader representation matching `buffering`.
    #[inline]
    pub(in crate::local) fn from_file(
        file: File,
        buffering: FileBuffering,
    ) -> Self {
        match buffering {
            FileBuffering::Unbuffered => Self::Unbuffered(file),
            FileBuffering::Buffered { capacity: None } => {
                Self::Buffered(BufReader::new(file))
            }
            FileBuffering::Buffered {
                capacity: Some(capacity),
            } => Self::Buffered(BufReader::with_capacity(capacity.get(), file)),
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
}

impl Read for LocalFileReaderInner {
    /// Reads through the active private representation.
    #[inline(always)]
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        match self {
            Self::Unbuffered(file) => file.read(buffer),
            Self::Buffered(reader) => reader.read(buffer),
        }
    }
}

impl Seek for LocalFileReaderInner {
    /// Seeks through the active private representation.
    #[inline(always)]
    fn seek(&mut self, position: SeekFrom) -> Result<u64> {
        match self {
            Self::Unbuffered(file) => file.seek(position),
            Self::Buffered(reader) => reader.seek(position),
        }
    }
}
