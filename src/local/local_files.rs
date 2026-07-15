// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public local filesystem utility namespace.

use std::convert::Infallible;
use std::fs::{self, File};
use std::io::Result;
use std::path::Path;

use crate::{
    FileReadOptions, FileWriteOptions, LocalAtomicWriteError, LocalCopyDirError,
    LocalCopyDirOptions, LocalCopyDirStats, LocalFileReader, LocalFileWriter,
};

use super::internal::LocalFileOperations;

/// File-system utility namespace.
///
/// This type cannot be instantiated. Use its associated methods for recurring
/// local filesystem operations such as opening files, creating parents,
/// recursively copying directories, and atomically replacing files.
///
/// # Examples
/// ```
/// use qubit_local_files::{LocalFiles, LocalTempDir};
///
/// let dir = LocalTempDir::with_prefix(Some("qubit-local-files-doc-"))?;
/// let path = dir.path().join("nested").join("data.txt");
///
/// LocalFiles::atomic_write(&path, b"payload")?;
/// assert_eq!(b"payload", std::fs::read(&path)?.as_slice());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct LocalFiles {
    _private: Infallible,
}

impl LocalFiles {
    /// Default number of attempts used when creating a random temporary entry.
    pub const DEFAULT_TEMP_FILE_RETRIES: usize = LocalFileOperations::DEFAULT_TEMP_FILE_RETRIES;

    /// Tests whether a path exists.
    ///
    /// # Parameters
    /// - `path`: Path to inspect.
    ///
    /// # Returns
    /// `true` when `path` exists and `false` when it is missing.
    ///
    /// # Errors
    /// Returns an I/O error when the filesystem cannot determine whether the
    /// path exists. Unlike [`Path::exists`], this method does not silently map
    /// inspection errors to `false`.
    #[inline(always)]
    pub fn exists<P>(path: P) -> Result<bool>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::exists(path)
    }

    /// Reads metadata for a local filesystem path.
    ///
    /// # Parameters
    /// - `path`: Path whose metadata should be read.
    ///
    /// # Returns
    /// Metadata reported by [`fs::metadata`].
    ///
    /// # Errors
    /// Returns the I/O error reported by the filesystem. Symbolic links are
    /// followed, matching [`fs::metadata`].
    #[inline(always)]
    pub fn metadata<P>(path: P) -> Result<fs::Metadata>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::metadata(path)
    }

    /// Lists the direct entries of a directory.
    ///
    /// # Parameters
    /// - `path`: Directory path to list.
    ///
    /// # Returns
    /// A directory iterator over direct children of `path`.
    ///
    /// # Errors
    /// Returns the I/O error reported by [`fs::read_dir`].
    #[inline(always)]
    pub fn list<P>(path: P) -> Result<fs::ReadDir>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::list(path)
    }

    /// Opens a local file for reading.
    ///
    /// The target must be a file. Directories and other non-file paths are
    /// rejected before returning the reader.
    ///
    /// # Parameters
    /// - `path`: File path to open.
    /// - `options`: Read options controlling buffering.
    ///
    /// # Returns
    /// A file reader matching `options`.
    ///
    /// # Errors
    /// Returns an I/O error when `path` cannot be inspected or opened, or when
    /// the target is not a file.
    #[inline(always)]
    pub fn open_reader<P>(path: P, options: FileReadOptions) -> Result<LocalFileReader>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::open_reader(path, options)
    }

    /// Opens a local file for writing.
    ///
    /// Whole-file durable replacement remains the responsibility of
    /// [`LocalFiles::atomic_write`] and [`LocalFiles::atomic_write_with`].
    ///
    /// # Parameters
    /// - `path`: File path to open.
    /// - `options`: Write options controlling parent creation, write mode, and
    ///   buffering.
    ///
    /// # Returns
    /// A file writer matching `options`.
    ///
    /// # Errors
    /// Returns an I/O error when parent directories cannot be created or the
    /// file cannot be opened with the requested mode.
    #[inline(always)]
    pub fn open_writer<P>(path: P, options: FileWriteOptions) -> Result<LocalFileWriter>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::open_writer(path, options)
    }

    /// Ensures that a directory exists.
    ///
    /// # Parameters
    /// - `path`: Directory path to create if missing.
    ///
    /// # Errors
    /// Returns an I/O error when the directory or one of its ancestors cannot
    /// be created.
    #[inline(always)]
    pub fn ensure_dir<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::ensure_dir(path)
    }

    /// Ensures that a path's parent directory exists.
    ///
    /// Parentless paths and paths whose parent is empty are accepted without
    /// creating any directory.
    ///
    /// # Parameters
    /// - `path`: File path whose parent directory should be created.
    ///
    /// # Errors
    /// Returns an I/O error when the parent directory or one of its ancestors
    /// cannot be created.
    #[inline(always)]
    pub fn ensure_parent<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::ensure_parent(path)
    }

    /// Computes the total size of regular files under a directory.
    ///
    /// The root path must be a directory. This method recursively sums regular
    /// file lengths and ignores symbolic links.
    ///
    /// # Parameters
    /// - `path`: Directory whose regular-file contents should be measured.
    ///
    /// # Returns
    /// Total byte length of regular files contained in the directory tree.
    ///
    /// # Errors
    /// Returns an I/O error when `path` cannot be inspected, is not a
    /// directory, or one of the directory entries cannot be read.
    #[inline(always)]
    pub fn dir_size<P>(path: P) -> Result<u64>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::dir_size(path)
    }

    /// Removes all entries from a directory while keeping the directory itself.
    ///
    /// Nested directories are removed recursively. Symbolic links are removed
    /// as links and are not followed.
    ///
    /// # Parameters
    /// - `path`: Directory to clean.
    ///
    /// # Errors
    /// Returns an I/O error when `path` cannot be read, is not a directory, or
    /// one of its entries cannot be removed.
    #[inline(always)]
    pub fn clean_dir<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::clean_dir(path)
    }

    /// Removes a file, directory, or symbolic link.
    ///
    /// Directories are removed recursively. Symbolic links, including Windows
    /// directory symbolic links, are removed as links and are not followed.
    ///
    /// # Parameters
    /// - `path`: Path to remove.
    ///
    /// # Errors
    /// Returns an I/O error when `path` cannot be inspected or removed.
    #[inline(always)]
    pub fn remove_any<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        LocalFileOperations::remove_any(path)
    }

    /// Recursively copies a directory tree.
    ///
    /// The source must be a directory. Existing entries and type conflicts are
    /// handled according to `options`. Symbolic links are rejected by default.
    /// Destinations inside a source tree and cycles introduced by followed
    /// symbolic links are rejected.
    ///
    /// When overwrite is enabled and a source file conflicts with a destination
    /// directory, the source is staged successfully before that directory is
    /// removed. The final remove-and-move sequence is not atomic: a commit
    /// failure after removal can leave the destination absent.
    ///
    /// # Parameters
    /// - `src`: Source directory.
    /// - `dst`: Destination directory.
    /// - `options`: Copy behavior options.
    ///
    /// # Returns
    /// Statistics describing copied files, created directories, copied bytes,
    /// and skipped entries.
    ///
    /// # Errors
    /// Returns [`LocalCopyDirError`] with the failed stage, source and
    /// destination paths, partial statistics, and native I/O source error when
    /// validation or an underlying filesystem operation fails.
    #[inline(always)]
    pub fn copy_dir_all_with<S, D>(
        src: S,
        dst: D,
        options: LocalCopyDirOptions,
    ) -> std::result::Result<LocalCopyDirStats, LocalCopyDirError>
    where
        S: AsRef<Path>,
        D: AsRef<Path>,
    {
        LocalFileOperations::copy_dir_all_with(src, dst, options)
    }

    /// Atomically writes bytes using a same-directory temporary file.
    ///
    /// Parent directories are created first. The temporary file is flushed and
    /// synced before it replaces the destination, after which the parent
    /// directory is synced where supported. A symbolic-link destination is
    /// replaced as a link rather than followed on platforms whose rename
    /// semantics provide that behavior.
    ///
    /// Before replacement, every error leaves the existing destination intact
    /// and removes the temporary file. After replacement, a parent-directory
    /// sync failure is reported even though the new destination is committed.
    /// This operation is not a multi-file transaction and does not coordinate
    /// concurrent writers.
    ///
    /// # Examples
    /// ```
    /// use qubit_local_files::{LocalFiles, LocalTempDir};
    ///
    /// let dir = LocalTempDir::with_prefix(Some("qubit-local-files-atomic-"))?;
    /// let path = dir.path().join("state").join("manifest.json");
    /// LocalFiles::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;
    /// assert_eq!(
    ///     br#"{"version":1,"complete":true}"#,
    ///     std::fs::read(&path)?.as_slice(),
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Parameters
    /// - `path`: Destination path.
    /// - `bytes`: Bytes to write.
    ///
    /// # Errors
    /// Returns [`LocalAtomicWriteError`] with the failed stage, temporary path,
    /// commit state, and native I/O source error.
    #[inline(always)]
    pub fn atomic_write<P, B>(path: P, bytes: B) -> std::result::Result<(), LocalAtomicWriteError>
    where
        P: AsRef<Path>,
        B: AsRef<[u8]>,
    {
        LocalFileOperations::atomic_write(path, bytes)
    }

    /// Atomically writes a file using caller-provided write logic.
    ///
    /// The callback receives the same-directory temporary file. After it
    /// returns successfully, the file is flushed, synced, closed, and moved
    /// over the destination before the parent directory is synced. An
    /// uncommitted temporary file is removed both on ordinary errors and while
    /// unwinding from a callback panic.
    ///
    /// # Parameters
    /// - `path`: Destination path.
    /// - `write`: Function that writes the desired contents into the temporary
    ///   file.
    ///
    /// # Errors
    /// Returns [`LocalAtomicWriteError`] with the failed stage, temporary path,
    /// commit state, and native I/O source error.
    ///
    /// # Panics
    /// Propagates a panic raised by `write` after closing and removing the
    /// uncommitted temporary file.
    #[inline(always)]
    pub fn atomic_write_with<P, F>(
        path: P,
        write: F,
    ) -> std::result::Result<(), LocalAtomicWriteError>
    where
        P: AsRef<Path>,
        F: FnOnce(&mut File) -> Result<()>,
    {
        LocalFileOperations::atomic_write_with(path, write)
    }
}
