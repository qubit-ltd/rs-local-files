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
///
/// A vectored read can return a short count. The count describes the ordered
/// prefix written across the supplied buffers; callers should use that count
/// instead of assuming every buffer was filled.
///
/// ```no_run
/// use std::io::IoSliceMut;
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut reader = filesystem.open_reader(Path::new("Cargo.toml"))?;
/// const FIRST_LEN: usize = 8;
/// const SECOND_LEN: usize = 64;
/// let mut first = [0_u8; FIRST_LEN];
/// let mut second = [0_u8; SECOND_LEN];
/// let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];
/// let count = reader.read_vectored(&mut buffers)?;
/// let mut prefix = Vec::new();
/// prefix.extend_from_slice(&buffers[0][..count.min(FIRST_LEN)]);
/// if count > FIRST_LEN {
///     prefix.extend_from_slice(&buffers[1][..count - FIRST_LEN]);
/// }
/// assert!(count <= FIRST_LEN + SECOND_LEN);
/// assert_eq!(prefix.len(), count);
/// # Ok(())
/// # }
/// ```
pub struct LocalFileReader {
    /// Open native file handle.
    file: File,
    /// Metadata observed from the same handle after it was opened.
    metadata: LocalFileMetadata,
}

impl std::fmt::Debug for LocalFileReader {
    /// Formats the reader without exposing native handle details.
    ///
    /// The debug representation includes the metadata captured from the
    /// retained handle and omits the operating-system-specific file handle.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter that receives the reader's debug representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalFileReader")
            .field("metadata", &self.metadata)
            .finish()
    }
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
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::ReaderMetadata)?;
        let metadata = LocalFileMetadata::from_native(&file.metadata()?);
        Ok(Self { file, metadata })
    }

    /// Returns the underlying native file handle.
    ///
    /// This is the same open file retained by the reader. Standard reads and
    /// seeks through this handle change the file position subsequently used by
    /// the reader. A handle returned by [`File::try_clone`] shares that file
    /// position as well; it does not provide an independent cursor. Callers
    /// that mix reader operations with operations on this handle or its clones
    /// must coordinate access to the shared position.
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
    ///
    /// Empty input, including an empty slice or a slice containing only empty
    /// buffers, returns `Ok(0)` without reading. Non-empty input may produce a
    /// short read or reach EOF; the returned count covers only the ordered
    /// prefix of the buffers and does not imply that all buffers were filled.
    /// On Windows, buffers are read sequentially. If a later read fails after
    /// earlier buffers received bytes, the accumulated count is returned and
    /// that later error is suppressed; an error before any progress is
    /// returned. Unix calls the standard file vectored-read operation directly
    /// and does not allocate adapter descriptors. Other platforms use the
    /// underlying vectored input operation; a single-buffer request uses a
    /// stack descriptor and multi-buffer requests may allocate temporary
    /// borrowed slice descriptors without copying buffer contents. Backend
    /// allocations are outside this adapter guarantee.
    ///
    /// # Parameters
    ///
    /// - `buffers`: destination buffers filled in order from the current file
    ///   offset.
    ///
    /// # Returns
    ///
    /// The number of bytes read into the ordered prefix of `buffers`.
    ///
    /// # Errors
    ///
    /// Returns an underlying read error when no bytes were read, or
    /// `InvalidInput` if the total destination length overflows. On platforms
    /// using the adapter, returns `OutOfMemory` if its multi-buffer slice
    /// descriptors cannot be allocated.
    pub fn read_vectored(&mut self, buffers: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        if buffers.iter().all(|buffer| buffer.is_empty()) {
            return Ok(0);
        }
        #[cfg(unix)]
        {
            buffers.iter().try_fold(0_usize, |total, buffer| {
                total
                    .checked_add(buffer.len())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "vectored read length overflow"))
            })?;
            #[cfg(test)]
            crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::Read)?;
            std::io::Read::read_vectored(&mut self.file, buffers)
        }
        #[cfg(not(unix))]
        {
            if let [buffer] = buffers {
                let mut slices = [&mut buffer[..]];
                return qubit_io::Input::read_vectored(self, &mut slices);
            }
            let mut slices = crate::vectored::byte_slices_mut(buffers)?;
            qubit_io::Input::read_vectored(self, &mut slices)
        }
    }
}

impl qubit_io::Input for LocalFileReader {
    /// Bytes read from the native file handle.
    type Item = u8;

    /// Reads into byte slices from the current native file offset.
    ///
    /// Empty input returns `Ok(0)`. A non-empty request may be short or reach
    /// EOF; the count covers the ordered prefix of the destination slices.
    /// Windows reads slices sequentially, retaining progress if a later read
    /// fails; other platforms delegate to the native vectored read operation.
    ///
    /// # Parameters
    ///
    /// - `buffers`: Mutable destination slices filled in order.
    ///
    /// # Returns
    ///
    /// The number of bytes written across the ordered prefix of `buffers`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reading fails before any bytes are transferred.
    fn read_vectored(&mut self, buffers: &mut [&mut [u8]]) -> io::Result<usize> {
        if buffers.iter().all(|buffer| buffer.is_empty()) {
            return Ok(0);
        }
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::Read)?;
        #[cfg(windows)]
        {
            crate::read::read_vectored_fallback(&mut self.file, buffers)
        }
        #[cfg(not(windows))]
        qubit_io::Input::read_vectored(&mut self.file, buffers)
    }

    /// Reads a caller-selected range of the output buffer.
    ///
    /// # Parameters
    ///
    /// - `output`: Destination storage.
    /// - `index`: Start of the requested range.
    /// - `count`: Maximum number of bytes to read.
    ///
    /// # Returns
    ///
    /// The number of bytes written to the requested range; reads may be short.
    ///
    /// # Errors
    ///
    /// Returns an I/O error from the retained native file handle.
    ///
    /// # Safety
    ///
    /// The caller must ensure `index..index + count` is within `output` and
    /// that calculating the range does not overflow.
    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::Read)?;
        qubit_io::Input::read(&mut self.file, &mut output[index..index + count])
    }
}

impl Seek for LocalFileReader {
    /// Moves the native file cursor and returns its new byte offset.
    ///
    /// # Parameters
    ///
    /// - `position`: Absolute, relative, or end-relative cursor position.
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
