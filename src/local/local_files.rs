// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public local filesystem utility namespace.

use std::convert::Infallible;
use std::fs;
use std::io::Result;
use std::path::Path;

use crate::{
    FileReadOptions,
    FileWriteOptions,
    LocalAtomicWriteError,
    LocalAtomicWriter,
    LocalCopyDirError,
    LocalCopyDirOptions,
    LocalCopyDirStats,
    LocalFileReader,
    LocalFileWriter,
};

use super::internal::{
    DEFAULT_TEMP_FILE_RETRIES as DEFAULT_TEMP_FILE_RETRIES_VALUE,
    clean_dir_path,
    copy_dir_all_with_paths,
    dir_size_path,
    ensure_dir_path,
    ensure_parent_path,
    open_reader_path,
    open_writer_path,
    remove_any_path,
};

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
/// let dir = LocalTempDir::with_prefix("qubit-local-files-doc-")?;
/// let path = dir.path().join("nested").join("data.txt");
///
/// LocalFiles::atomic_write(&path, b"payload")?;
/// assert_eq!(b"payload", std::fs::read(&path)?.as_slice());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct LocalFiles {
    /// Uninhabited field that prevents construction of this namespace type.
    _private: Infallible,
}

impl LocalFiles {
    /// Default number of attempts used when creating a random temporary entry.
    pub const DEFAULT_TEMP_FILE_RETRIES: usize =
        DEFAULT_TEMP_FILE_RETRIES_VALUE;

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
        path.as_ref().try_exists()
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
        fs::metadata(path)
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
        fs::read_dir(path)
    }

    /// Opens a local file for reading.
    ///
    /// The target must be a regular file. Directories and special filesystem
    /// resources are rejected without returning a reader.
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
    /// the target is not a regular file.
    #[inline(always)]
    pub fn open_reader<P>(
        path: P,
        options: FileReadOptions,
    ) -> Result<LocalFileReader>
    where
        P: AsRef<Path>,
    {
        open_reader_path(path.as_ref(), options)
    }

    /// Opens a local file for writing.
    ///
    /// Whole-file durable replacement remains the responsibility of
    /// [`LocalFiles::atomic_write`] and [`LocalFiles::atomic_write_with`].
    /// The target must be a regular file or a path that the selected mode can
    /// create; directories and special filesystem resources are rejected.
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
    /// file cannot be opened with the requested mode, or the target is not a
    /// regular file.
    #[inline(always)]
    pub fn open_writer<P>(
        path: P,
        options: FileWriteOptions,
    ) -> Result<LocalFileWriter>
    where
        P: AsRef<Path>,
    {
        open_writer_path(path.as_ref(), options)
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
        ensure_dir_path(path.as_ref())
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
        ensure_parent_path(path.as_ref())
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
    /// directory, one of the directory entries cannot be read, or the
    /// aggregate size exceeds [`u64::MAX`]. Overflow is reported as
    /// [`std::io::ErrorKind::InvalidData`].
    #[inline(always)]
    pub fn dir_size<P>(path: P) -> Result<u64>
    where
        P: AsRef<Path>,
    {
        dir_size_path(path.as_ref())
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
        clean_dir_path(path.as_ref())
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
        remove_any_path(path.as_ref())
    }

    /// Recursively copies a directory tree.
    ///
    /// The source must be a directory. Existing entries and type conflicts are
    /// handled according to `options`. Symbolic links are rejected by default.
    /// Destinations inside a source tree and cycles introduced by followed
    /// symbolic links are rejected.
    /// On Unix, new or replaced files use mode `0o600` and newly created
    /// directories use mode `0o700`, subject to a more restrictive process
    /// umask, unless source permissions are explicitly preserved.
    ///
    /// When [`LocalCopyTypeConflictPolicy::Replace`](crate::LocalCopyTypeConflictPolicy::Replace)
    /// is enabled and a source file conflicts with a destination directory, the
    /// source is staged successfully before that directory is removed. The
    /// final remove-and-move sequence is not atomic: a commit failure after
    /// removal can leave the destination absent.
    ///
    /// Relative source and destination paths are bound to the process current
    /// directory when this operation begins. Later current-directory changes
    /// do not redirect traversal, staging, or commit.
    ///
    /// This operation is not a tree-level transaction. If it fails,
    /// directories and files created or committed before the failure remain in
    /// the destination and no rollback is attempted. Type-conflict replacement
    /// may recursively remove an existing destination directory before a later
    /// operation fails.
    ///
    /// Each file's regular-file type and optional source permissions come from
    /// the same opened handle used to copy its bytes. Unix opens use
    /// `O_NOFOLLOW` when links are disabled; Windows rejects name-surrogate
    /// reparse handles. Directory traversal, destination reinspection, and
    /// destructive replacement remain separate path-based operations, so the
    /// symbolic-link policy is not a sandbox boundary against concurrent tree
    /// mutation. Use descriptor- or capability-relative filesystem APIs when
    /// containment must resist concurrent path replacement.
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
    /// destination paths, partial statistics, optional staging path, optional
    /// secondary cleanup error, and native I/O source error when validation or
    /// an underlying filesystem operation fails.
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
        copy_dir_all_with_paths(src.as_ref(), dst.as_ref(), options)
    }

    /// Begins a streaming same-directory atomic file replacement.
    ///
    /// The returned writer owns a staging file. The destination is replaced
    /// only by [`LocalAtomicWriter::commit`]; aborting or dropping the writer
    /// leaves the destination unchanged.
    ///
    /// The destination must be absent or a regular file. Symbolic links,
    /// directories, sockets, FIFOs, devices, and other special files are
    /// rejected with [`std::io::ErrorKind::InvalidInput`].
    ///
    /// Existing Unix metadata is read from an opened destination during
    /// commit; Windows uses `ReplaceFileW` to merge destination metadata during
    /// replacement. Preservation is strict: metadata or ACL merge failures
    /// abort instead of silently degrading protection. A destination that was
    /// absent when this method returns is installed without replacing a
    /// concurrent creator. The final inspection and replacement remain
    /// separate path operations; use
    /// [`crate::LocalRoot`] when containment must be anchored to an opened
    /// directory capability.
    ///
    /// # Parameters
    /// - `path`: Destination path to replace on commit.
    ///
    /// # Returns
    /// A streaming writer for the private staging file.
    ///
    /// # Errors
    /// Returns a structured error when parent preparation, destination
    /// inspection, or staging-file creation fails.
    #[inline(always)]
    pub fn begin_atomic_write<P>(
        path: P,
    ) -> std::result::Result<LocalAtomicWriter, LocalAtomicWriteError>
    where
        P: AsRef<Path>,
    {
        LocalAtomicWriter::new(path.as_ref())
    }

    /// Atomically writes bytes using a same-directory temporary file.
    ///
    /// Parent directories are created first. The temporary file is flushed and
    /// synced before it replaces the destination. The destination parent and
    /// the parents of directory entries created by this call are then synced
    /// from deepest to shallowest where supported. The destination must be
    /// absent or a regular file. Symbolic links, directories, sockets, FIFOs,
    /// devices, and other special files are rejected with
    /// [`std::io::ErrorKind::InvalidInput`].
    ///
    /// Existing destination metadata is captured at commit time. Linux and
    /// Android preserve uid, gid, mode, and descriptor-visible extended
    /// attributes; macOS preserves uid, gid, mode, ACLs, and extended
    /// attributes; FreeBSD preserves uid, gid, mode, ACLs, and user/system
    /// extattrs. Windows uses strict `ReplaceFileW` metadata merging. Any
    /// preservation failure aborts. New destinations use native no-replace
    /// installation; on Unix their initial mode is `0o600`, subject to a more
    /// restrictive process umask.
    ///
    /// Android and FreeBSD are compile-only targets: CI checks that their code
    /// compiles, but does not execute runtime filesystem validation there.
    ///
    /// Failures classified as [`crate::LocalAtomicDestinationState::Unchanged`]
    /// do not modify the destination and attempt to remove the temporary file.
    /// Cleanup is best-effort because its error cannot replace the primary
    /// structured error. `Missing` and `Indeterminate` retain any
    /// still-existing staging entry for recovery; `Replaced` can report a
    /// later durability failure after installation. This operation is not a
    /// multi-file transaction and does not coordinate concurrent writers.
    /// The final type inspection and replacement are separate
    /// path-based operations, so this API is not a sandbox boundary against
    /// concurrent path replacement; use [`crate::LocalRoot`] when
    /// descriptor-relative containment is required. A relative destination is
    /// bound to the process current directory when the atomic writer is
    /// created. On Windows, replacement does not add a verbatim-path prefix, so
    /// native path-length and verbatim-path semantics apply.
    ///
    /// # Examples
    /// ```
    /// use qubit_local_files::{LocalFiles, LocalTempDir};
    ///
    /// let dir = LocalTempDir::with_prefix("qubit-local-files-atomic-")?;
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
    /// destination state, native I/O source error, and any secondary staging
    /// cleanup error.
    #[inline(always)]
    pub fn atomic_write<P, B>(
        path: P,
        bytes: B,
    ) -> std::result::Result<(), LocalAtomicWriteError>
    where
        P: AsRef<Path>,
        B: AsRef<[u8]>,
    {
        Self::begin_atomic_write(path)?.write_bytes(bytes.as_ref())
    }

    /// Atomically writes a file using caller-provided write logic.
    ///
    /// The callback receives a non-cloneable [`LocalAtomicWriter`] capability
    /// that implements [`std::io::Write`] but deliberately exposes neither
    /// [`std::io::Seek`] nor the underlying file or raw handle. After it
    /// returns successfully, the staging file is flushed, synced, closed, and
    /// moved over the destination before the parent directory is synced. An
    /// uncommitted staging file is closed and best-effort removed on callback
    /// errors and while unwinding from a callback panic. Commit failures follow
    /// [`LocalAtomicWriteError::destination_state`]: only `Unchanged` attempts
    /// automatic cleanup, while recovery-sensitive states retain any
    /// still-existing staging entry. A cleanup failure cannot replace the
    /// original error or panic and may therefore leave the staging path behind.
    /// Parent-chain synchronization and new-file permission behavior are the
    /// same as for [`Self::atomic_write`], including rejection of symbolic
    /// links and other non-regular destinations and commit-time type
    /// reinspection.
    ///
    /// The callback receives a guarded writer rather than a cloneable file
    /// handle:
    ///
    /// ```compile_fail
    /// use qubit_local_files::{LocalFiles, LocalTempDir};
    ///
    /// let dir = LocalTempDir::new()?;
    /// let path = dir.path().join("state.bin");
    /// let mut escaped = None;
    /// LocalFiles::atomic_write_with(&path, |writer| {
    ///     escaped = Some(writer.try_clone()?);
    ///     Ok(())
    /// })?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Parameters
    /// - `path`: Destination path.
    /// - `write`: Function that writes the desired contents through the guarded
    ///   staging writer.
    ///
    /// # Errors
    /// Returns [`LocalAtomicWriteError`] with the failed stage, temporary path,
    /// destination state, native I/O source error, and any secondary staging
    /// cleanup error.
    ///
    /// # Panics
    /// Propagates a panic raised by `write` after closing and attempting to
    /// remove the uncommitted temporary file. Cleanup is best-effort, so the
    /// staging path may remain if removal fails during unwinding.
    #[inline(always)]
    pub fn atomic_write_with<P, F>(
        path: P,
        write: F,
    ) -> std::result::Result<(), LocalAtomicWriteError>
    where
        P: AsRef<Path>,
        F: FnOnce(&mut LocalAtomicWriter) -> Result<()>,
    {
        Self::begin_atomic_write(path)?.write_with(write)
    }
}
