// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Automatically cleaned local temporary files.

use std::fs::{
    self,
    File,
};
use std::io::{
    Error,
    ErrorKind,
    IoSlice,
    Result,
    Seek,
    SeekFrom,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};

use log::warn;

use crate::{
    LocalFiles,
    LocalPersistError,
    LocalPersistOptions,
    LocalPersistStage,
};

use super::internal::{
    absolute_path,
    create_temp_file_in_dir,
    move_file_without_replacing,
    replace_file,
};

/// Temporary file that is removed automatically unless kept or persisted.
///
/// `LocalTempFile` owns both the temporary file path and its open file handle.
/// It implements [`Write`] and [`Seek`], and the handle is closed before the
/// path is removed, kept, or persisted. Use
/// [`LocalTempFile::keep`] to keep the file at its generated path, or
/// [`LocalTempFile::persist`] to move it to a final path.
/// Relative creation directories are bound to the process current directory
/// at creation time. [`LocalTempFile::path`], [`LocalTempFile::keep`],
/// [`LocalTempFile::persist`], and [`LocalTempFile::persist_with`] expose
/// stable absolute paths that remain directly usable after later
/// current-directory changes.
///
/// Cleanup and persistence are bound to the generated path name rather than an
/// immutable filesystem-entry identity. Custom parent directories must belong
/// to a trusted namespace: a concurrent replacement of that name can redirect
/// the later cleanup or persistence operation. This guard is not a security
/// boundary against concurrent namespace mutation.
///
/// Cleanup performed from `Drop` is best-effort. If removal fails, the failure
/// is reported through the `log` facade at warning level and the program is not
/// panicked.
///
/// The guard must be retained until the file is kept, persisted, or cleaned:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::LocalTempFile;
///
/// let temporary_file = LocalTempFile::new()?;
/// temporary_file;
/// # Ok::<(), std::io::Error>(())
/// ```
#[must_use = "dropping the temporary-file guard removes its file"]
#[derive(Debug)]
pub struct LocalTempFile {
    /// Absolute generated path while cleanup remains armed.
    path: Option<PathBuf>,
    /// Original unbuffered file handle until explicitly closed.
    file: Option<File>,
}

impl LocalTempFile {
    /// Creates a temporary file in the process temporary directory.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created or a unique temporary file cannot be created.
    #[inline(always)]
    pub fn new() -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            None,
            None,
            LocalFiles::DEFAULT_TEMP_ENTRY_RETRIES,
        )
    }

    /// Creates a temporary file with a custom prefix in the process temporary
    /// directory.
    ///
    /// # Parameters
    /// - `prefix`: File-name prefix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `prefix` is not a safe file-name fragment, or a unique
    /// temporary file cannot be created.
    #[inline(always)]
    pub fn with_prefix(prefix: &str) -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            Some(prefix),
            None,
            LocalFiles::DEFAULT_TEMP_ENTRY_RETRIES,
        )
    }

    /// Creates a temporary file with a custom suffix in the process temporary
    /// directory.
    ///
    /// The default random prefix is retained.
    ///
    /// # Parameters
    /// - `suffix`: File-name suffix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `suffix` is not a safe file-name fragment, or a unique
    /// temporary file cannot be created.
    #[inline(always)]
    pub fn with_suffix(suffix: &str) -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            None,
            Some(suffix),
            LocalFiles::DEFAULT_TEMP_ENTRY_RETRIES,
        )
    }

    /// Creates a temporary file with custom prefix and suffix in the process
    /// temporary directory.
    ///
    /// # Parameters
    /// - `prefix`: File-name prefix.
    /// - `suffix`: File-name suffix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `prefix` or `suffix` is not a safe file-name fragment, or a
    /// unique temporary file cannot be created.
    #[inline(always)]
    pub fn with_affixes(prefix: &str, suffix: &str) -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            Some(prefix),
            Some(suffix),
            LocalFiles::DEFAULT_TEMP_ENTRY_RETRIES,
        )
    }

    /// Creates a temporary file in the specified directory.
    ///
    /// `dir` must belong to a trusted namespace. The returned guard manages the
    /// generated path name and cannot prevent another actor from replacing that
    /// directory entry before cleanup or persistence.
    ///
    /// # Parameters
    /// - `dir`: Parent directory in which the temporary file is created.
    /// - `prefix`: Optional file-name prefix.
    /// - `suffix`: Optional file-name suffix.
    /// - `max_tries`: Maximum number of random names to try.
    ///
    /// # Errors
    /// Returns an I/O error when `dir` cannot be created, `prefix` or `suffix`
    /// is not a safe file-name fragment, the retry limit is zero, all generated
    /// names collide, or file creation fails.
    pub fn in_dir<P>(
        dir: P,
        prefix: Option<&str>,
        suffix: Option<&str>,
        max_tries: usize,
    ) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        Self::in_directory(dir.as_ref(), prefix, suffix, max_tries)
    }

    /// Creates a temporary file from a borrowed directory path.
    fn in_directory(
        dir: &Path,
        prefix: Option<&str>,
        suffix: Option<&str>,
        max_tries: usize,
    ) -> Result<Self> {
        let operation_dir = absolute_path(dir)?;
        let (path, file) =
            create_temp_file_in_dir(&operation_dir, prefix, suffix, max_tries)?;
        Ok(Self {
            path: Some(path),
            file: Some(file),
        })
    }

    /// Returns the absolute temporary file path.
    ///
    /// # Returns
    /// Borrowed absolute path managed by this temporary file.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("temporary file path has already been released")
    }

    /// Tests whether the temporary file path still exists.
    ///
    /// # Returns
    /// `true` when the path exists and `false` when it is missing.
    ///
    /// # Errors
    /// Returns an I/O error when the filesystem cannot determine whether the
    /// path exists. Unlike [`Path::exists`], this method does not silently map
    /// inspection errors to `false`.
    #[inline(always)]
    pub fn exists(&self) -> Result<bool> {
        LocalFiles::exists(self.path())
    }

    /// Reads metadata for the temporary file path.
    ///
    /// # Returns
    /// Metadata for the temporary file path.
    ///
    /// # Errors
    /// Returns the I/O error reported by [`fs::metadata`].
    #[inline(always)]
    pub fn metadata(&self) -> Result<fs::Metadata> {
        LocalFiles::metadata(self.path())
    }

    /// Returns the owned file handle.
    ///
    /// # Returns
    /// Borrowed file handle while this temporary file is open.
    ///
    /// # Errors
    /// Returns [`ErrorKind::NotFound`] after [`LocalTempFile::close`] closes
    /// the handle.
    #[inline(always)]
    pub fn as_file(&self) -> Result<&File> {
        self.file.as_ref().ok_or_else(closed_file_error)
    }

    /// Returns the owned file handle mutably.
    ///
    /// # Returns
    /// Mutable borrowed file handle while this temporary file is open.
    ///
    /// # Errors
    /// Returns [`ErrorKind::NotFound`] after [`LocalTempFile::close`] closes
    /// the handle.
    #[inline(always)]
    pub fn as_file_mut(&mut self) -> Result<&mut File> {
        self.file.as_mut().ok_or_else(closed_file_error)
    }

    /// Closes the unbuffered temporary file handle while keeping cleanup
    /// active.
    ///
    /// The guard owns a raw [`File`] rather than a userspace buffer, so this
    /// operation only drops the handle. It does not call [`File::sync_all`] and
    /// does not provide a durability guarantee. Call `sync_all` through
    /// [`LocalTempFile::as_file_mut`] before closing when durable storage is
    /// required.
    #[inline(always)]
    pub fn close(&mut self) {
        drop(self.file.take());
    }

    /// Removes the temporary file immediately.
    ///
    /// This consumes the guard and disables the later best-effort cleanup in
    /// `Drop` after removal succeeds. If removal fails, the guard still owns
    /// the path until it is dropped. Removal applies to the filesystem entry
    /// currently stored at the generated path name.
    ///
    /// # Errors
    /// Returns an I/O error when removing the file fails.
    pub fn cleanup(mut self) -> Result<()> {
        self.close();
        let path = self.path().to_path_buf();
        fs::remove_file(&path)?;
        let _ = self.path.take();
        Ok(())
    }

    /// Keeps the temporary file at its generated path.
    ///
    /// This consumes the guard, closes the file, and disables automatic
    /// cleanup.
    ///
    /// # Returns
    /// The absolute generated temporary file path.
    ///
    /// Ignoring the returned path is rejected:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// use qubit_local_files::LocalTempFile;
    ///
    /// let temporary_file = LocalTempFile::new()?;
    /// temporary_file.keep();
    /// # Ok::<(), std::io::Error>(())
    /// ```
    #[must_use = "keeping the temporary file disables automatic cleanup; retain the returned path"]
    #[inline]
    pub fn keep(mut self) -> PathBuf {
        self.close();
        self.path
            .take()
            .expect("temporary file path has already been released")
    }

    /// Moves the temporary file to a final path without overwriting.
    ///
    /// The file is closed before target resolution and moving. Parent
    /// directories for `target` are created before moving. Existing targets
    /// are rejected by the move operation instead of by a separate metadata
    /// precheck. Use
    /// [`LocalTempFile::persist_with`] and [`LocalPersistOptions`] when
    /// overwriting is intended. If persistence fails, the returned
    /// [`LocalPersistError`] retains this guard in its closed state so the
    /// caller can retry persistence, keep the path, inspect path metadata, or
    /// explicitly clean up the temporary file. The retained guard cannot read,
    /// write, or seek through its original handle.
    ///
    /// Native no-replace persistence is available on Linux, macOS, and
    /// Windows. Other targets return [`std::io::ErrorKind::Unsupported`] and
    /// retain this guard in the error. The same platform restriction applies
    /// to recursive-copy file commits using the `Fail` or `Skip` conflict
    /// policy because those operations share the no-replace primitive.
    ///
    /// Persistence uses a native move or rename and does not fall back to
    /// copying and deleting. Moving across filesystems can therefore fail with
    /// `EXDEV` on Unix or a platform-equivalent error.
    /// The source is the filesystem entry currently stored at the generated
    /// path name; persistence is not an identity-bound operation.
    /// A relative target is bound to the process current directory when this
    /// method begins, and the returned path is absolute. On Windows, no
    /// verbatim-path prefix is added, so native path-length and verbatim-path
    /// semantics still apply.
    ///
    /// # Parameters
    /// - `target`: Final file path.
    ///
    /// # Returns
    /// The absolute final file path.
    ///
    /// # Errors
    /// Returns [`LocalPersistError`] with the failure stage, requested target,
    /// optional resolved target, native error, and this retained guard when
    /// target resolution, parent preparation, or installation fails.
    #[inline(always)]
    pub fn persist<P>(
        self,
        target: P,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>>
    where
        P: AsRef<Path>,
    {
        self.persist_path_with(target.as_ref(), LocalPersistOptions::default())
    }

    /// Moves the temporary file to a final path using persistence options.
    ///
    /// The file is closed before target resolution and moving the path. On any
    /// failure, the returned [`LocalPersistError`] retains the guard in its
    /// closed state; its path operations remain available, but its original
    /// handle cannot be used for reading, writing, or seeking. Parent
    /// directories for `target` are created before moving. When
    /// `options.overwrites()` is `false`, existing targets are rejected by the
    /// move operation. When
    /// `options.overwrites()` is `true`, an existing target file may be
    /// replaced.
    /// Native no-replace persistence is available on Linux, macOS, and
    /// Windows. On other targets, disabling overwrite returns
    /// [`std::io::ErrorKind::Unsupported`] and retains this guard in the error.
    /// Overwrite persistence continues to use the platform's ordinary
    /// replacement primitive and is not subject to that no-replace matrix.
    /// Persistence uses a native move or rename and does not fall back to
    /// copying and deleting, so cross-filesystem moves can fail with `EXDEV` on
    /// Unix or a platform-equivalent error. Replacing an existing target keeps
    /// the temporary file's metadata and does not preserve the replaced
    /// target's metadata. Use [`LocalFiles::atomic_write`] when replacing
    /// contents while strictly preserving supported platform-native metadata
    /// is required.
    /// The source is the filesystem entry currently stored at the generated
    /// path name; persistence is not an identity-bound operation.
    /// A relative target is bound to the process current directory when this
    /// method begins, and the returned path is absolute. On Windows, no
    /// verbatim-path prefix is added, so native path-length and verbatim-path
    /// semantics still apply.
    ///
    /// # Parameters
    /// - `target`: Final file path.
    /// - `options`: Persistence behavior options.
    ///
    /// # Returns
    /// The absolute final file path.
    ///
    /// # Errors
    /// Returns [`LocalPersistError`] with target and stage context while
    /// retaining this guard when resolution, parent preparation, or
    /// installation fails, including no-replace conflicts and unsupported
    /// native operations.
    pub fn persist_with<P>(
        self,
        target: P,
        options: LocalPersistOptions,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>>
    where
        P: AsRef<Path>,
    {
        self.persist_path_with(target.as_ref(), options)
    }

    /// Persists this file using one borrowed target path.
    ///
    /// The original handle is closed before resolving `target`. Every error
    /// therefore retains a closed guard whose path ownership remains active.
    ///
    /// # Parameters
    /// - `target`: Borrowed final file path.
    /// - `options`: Persistence behavior options.
    ///
    /// # Returns
    /// The absolute final file path.
    ///
    /// # Errors
    /// Returns [`LocalPersistError`] with the closed guard when target
    /// resolution, parent preparation, or installation fails.
    ///
    /// # Panics
    /// Panics if the temporary path was released before this method begins.
    fn persist_path_with(
        mut self,
        target: &Path,
        options: LocalPersistOptions,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>> {
        self.close();
        let requested_target = target.to_path_buf();
        let target = match absolute_path(&requested_target) {
            Ok(path) => path,
            Err(error) => {
                return Err(LocalPersistError::new(
                    error,
                    self,
                    requested_target,
                    None,
                    LocalPersistStage::ResolveTarget,
                ));
            }
        };
        if let Err(error) = LocalFiles::ensure_parent(&target) {
            return Err(LocalPersistError::new(
                error,
                self,
                requested_target,
                Some(target),
                LocalPersistStage::PrepareParent,
            ));
        }
        let move_result = {
            let source = self
                .path
                .as_ref()
                .expect("temporary file path has already been released");
            if options.overwrites() {
                replace_file(source, &target)
            } else {
                move_file_without_replacing(source, &target)
            }
        };
        if let Err(error) = move_result {
            return Err(LocalPersistError::new(
                error,
                self,
                requested_target,
                Some(target),
                LocalPersistStage::InstallDestination,
            ));
        }
        let _ = self.path.take();
        Ok(target)
    }
}

impl Write for LocalTempFile {
    /// Writes bytes through the owned temporary file handle.
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.as_file_mut()?.write(buffer)
    }

    /// Writes bytes from multiple buffers through the owned file handle.
    #[inline(always)]
    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> Result<usize> {
        self.as_file_mut()?.write_vectored(buffers)
    }

    /// Flushes the owned temporary file handle.
    #[inline(always)]
    fn flush(&mut self) -> Result<()> {
        self.as_file_mut()?.flush()
    }
}

impl Seek for LocalTempFile {
    /// Seeks the owned temporary file handle.
    #[inline(always)]
    fn seek(&mut self, position: SeekFrom) -> Result<u64> {
        self.as_file_mut()?.seek(position)
    }
}

impl Drop for LocalTempFile {
    /// Closes and removes the temporary file unless ownership has been
    /// released.
    fn drop(&mut self) {
        self.close();
        if let Some(path) = self.path.take()
            && let Err(error) = fs::remove_file(&path)
        {
            warn!(
                "failed to remove temporary file {}: {}",
                path.display(),
                error
            );
        }
    }
}

/// Creates the error returned when a temporary file handle is closed.
///
/// # Returns
/// An [`ErrorKind::NotFound`] error describing the closed handle.
#[inline]
fn closed_file_error() -> Error {
    Error::new(ErrorKind::NotFound, "temporary file handle is closed")
}
