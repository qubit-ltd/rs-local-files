// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
// Covered by host and rooted reader integration tests.

use std::fs::File;
use std::io;
use std::io::IoSliceMut;
use std::io::Seek;
use std::io::SeekFrom;

use crate::LocalFileMetadata;

/// Owned synchronous reader for an opened native regular file.
///
/// # Examples
///
/// ```compile_fail
/// fn require_read<T: std::io::Read>() {}
/// require_read::<qubit_local_files::LocalFileReader>();
/// ```
///
/// ```no_run
/// use qubit_io::Streams;
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut reader = filesystem.open_reader(Path::new("Cargo.toml"))?;
/// let mut bytes = Vec::new();
/// Streams::copy_input_to_output(&mut reader, &mut bytes)?;
/// let contents = String::from_utf8(bytes)?;
/// assert!(!contents.is_empty());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct LocalFileReader {
    /// Open native file handle.
    file: File,
    /// Metadata observed from the same handle after it was opened.
    metadata: LocalFileMetadata,
}

impl LocalFileReader {
    /// Wraps an already validated native regular-file handle.
    ///
    /// # Parameters
    ///
    /// - `file`: Open native file handle.
    ///
    /// Takes ownership and captures metadata from the retained handle.
    /// Propagates metadata errors, closing the supplied handle on failure.
    pub(crate) fn from_file(file: File) -> io::Result<Self> {
        let metadata = LocalFileMetadata::from_native(&file.metadata()?);
        Ok(Self { file, metadata })
    }

    /// Returns the underlying native file handle.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
    pub const fn as_file(&self) -> &File {
        &self.file
    }

    /// Returns metadata captured from this reader's retained handle.
    #[must_use = "the retained-handle metadata should be inspected"]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
    pub const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }
}

impl LocalFileReader {
    /// Reads bytes into multiple buffers from the current offset.
    pub fn read_vectored(&mut self, buffers: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        if buffers.iter().all(|buffer| buffer.is_empty()) {
            return Ok(0);
        }
        let mut slices = crate::vectored::byte_slices_mut(buffers)?;
        qubit_io::Input::read_vectored(self, &mut slices)
    }
}

impl qubit_io::Input for LocalFileReader {
    type Item = u8;

    fn read_vectored(&mut self, buffers: &mut [&mut [u8]]) -> io::Result<usize> {
        if buffers.iter().all(|buffer| buffer.is_empty()) {
            return Ok(0);
        }
        #[cfg(windows)]
        {
            let mut slices = crate::vectored::io_slices_mut(buffers)?;
            crate::read::read_vectored_fallback(&mut self.file, &mut slices)
        }
        #[cfg(not(windows))]
        qubit_io::Input::read_vectored(&mut self.file, buffers)
    }

    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        qubit_io::Input::read(&mut self.file, &mut output[index..index + count])
    }
}

impl Seek for LocalFileReader {
    /// Moves the native file cursor and returns its new byte offset.
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
