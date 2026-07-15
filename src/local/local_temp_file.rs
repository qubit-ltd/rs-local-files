// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use log::warn;

use crate::{LocalFiles, LocalPersistError, LocalPersistOptions};

use super::internal::{create_temp_file_in_dir, move_file_without_replacing, replace_file};

/// Temporary file that is removed automatically unless kept or persisted.
///
/// `LocalTempFile` owns both the temporary file path and its open file handle.
/// It implements [`Write`] and [`Seek`], and the handle is closed before the
/// path is removed, kept, or persisted. Use
/// [`LocalTempFile::keep`] to keep the file at its generated path, or
/// [`LocalTempFile::persist`] to move it to a final path.
///
/// Cleanup performed from `Drop` is best-effort. If removal fails, the failure
/// is reported through the `log` facade at warning level and the program is not
/// panicked.
#[derive(Debug)]
pub struct LocalTempFile {
    path: Option<PathBuf>,
    file: Option<File>,
}

impl LocalTempFile {
    /// Creates a temporary file in the process temporary directory.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created or a unique temporary file cannot be created.
    #[inline]
    pub fn new() -> Result<Self> {
        Self::with_name(None, None)
    }

    /// Creates a temporary file in the process temporary directory.
    ///
    /// # Parameters
    /// - `prefix`: Optional file-name prefix.
    /// - `suffix`: Optional file-name suffix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `prefix` or `suffix` is not a safe file-name fragment, or a
    /// unique temporary file cannot be created.
    #[inline]
    pub fn with_name(prefix: Option<&str>, suffix: Option<&str>) -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            prefix,
            suffix,
            LocalFiles::DEFAULT_TEMP_FILE_RETRIES,
        )
    }

    /// Creates a temporary file in the specified directory.
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
        let (path, file) = create_temp_file_in_dir(dir.as_ref(), prefix, suffix, max_tries)?;
        Ok(Self {
            path: Some(path),
            file: Some(file),
        })
    }

    /// Returns the temporary file path.
    ///
    /// # Returns
    /// Borrowed path managed by this temporary file.
    #[inline]
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
    #[inline]
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
    #[inline]
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
    #[inline]
    pub fn close(&mut self) {
        drop(self.file.take());
    }

    /// Removes the temporary file immediately.
    ///
    /// This consumes the guard and disables the later best-effort cleanup in
    /// `Drop` after removal succeeds. If removal fails, the guard still owns
    /// the path until it is dropped.
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
    /// The generated temporary file path.
    pub fn keep(mut self) -> PathBuf {
        self.close();
        self.path
            .take()
            .expect("temporary file path has already been released")
    }

    /// Moves the temporary file to a final path without overwriting.
    ///
    /// The file is closed before moving. Parent directories for
    /// `target` are created before moving. Existing targets are rejected by the
    /// move operation instead of by a separate metadata precheck. Use
    /// [`LocalTempFile::persist_with`] and [`LocalPersistOptions`] when
    /// overwriting is intended. If persistence fails, the returned
    /// [`LocalPersistError`] retains this guard so the caller can retry, keep,
    /// inspect, or explicitly clean up the temporary file.
    ///
    /// Persistence uses a native move or rename and does not fall back to
    /// copying and deleting. Moving across filesystems can therefore fail with
    /// `EXDEV` on Unix or a platform-equivalent error.
    ///
    /// # Parameters
    /// - `target`: Final file path.
    ///
    /// # Returns
    /// The final file path.
    ///
    /// # Errors
    /// Returns [`LocalPersistError`] when the parent directory cannot be
    /// created, the target already exists, or the temporary file cannot be
    /// moved to `target`.
    #[inline]
    pub fn persist<P>(self, target: P) -> std::result::Result<PathBuf, LocalPersistError<Self>>
    where
        P: AsRef<Path>,
    {
        self.persist_with(target, LocalPersistOptions::default())
    }

    /// Moves the temporary file to a final path using persistence options.
    ///
    /// The file is closed before moving the path. Parent directories for
    /// `target` are created before moving. When
    /// `options.overwrite` is `false`, existing targets are rejected by the
    /// move operation. When
    /// `options.overwrite` is `true`, an existing target file may be replaced.
    /// Persistence uses a native move or rename and does not fall back to
    /// copying and deleting, so cross-filesystem moves can fail with `EXDEV` on
    /// Unix or a platform-equivalent error. Replacing an existing target keeps
    /// the temporary file's permissions and does not preserve the replaced
    /// target's permissions. Use [`LocalFiles::atomic_write`] when replacing
    /// contents while preserving existing regular-file permissions is
    /// required.
    ///
    /// # Parameters
    /// - `target`: Final file path.
    /// - `options`: Persistence behavior options.
    ///
    /// # Returns
    /// The final file path.
    ///
    /// # Errors
    /// Returns [`LocalPersistError`] retaining this guard when the parent
    /// directory cannot be created, the target already exists while
    /// overwriting is disabled, or the temporary file cannot be moved to
    /// `target`.
    pub fn persist_with<P>(
        mut self,
        target: P,
        options: LocalPersistOptions,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>>
    where
        P: AsRef<Path>,
    {
        self.close();
        let target = target.as_ref().to_path_buf();
        if let Err(error) = LocalFiles::ensure_parent(&target) {
            return Err(LocalPersistError::new(error, self));
        }
        let move_result = {
            let source = self
                .path
                .as_ref()
                .expect("temporary file path has already been released");
            if options.overwrite {
                replace_file(source, &target)
            } else {
                move_file_without_replacing(source, &target)
            }
        };
        if let Err(error) = move_result {
            return Err(LocalPersistError::new(error, self));
        }
        let _ = self.path.take();
        Ok(target)
    }
}

impl Write for LocalTempFile {
    /// Writes bytes through the owned temporary file handle.
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.as_file_mut()?.write(buffer)
    }

    /// Flushes the owned temporary file handle.
    fn flush(&mut self) -> Result<()> {
        self.as_file_mut()?.flush()
    }
}

impl Seek for LocalTempFile {
    /// Seeks the owned temporary file handle.
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
fn closed_file_error() -> Error {
    Error::new(ErrorKind::NotFound, "temporary file handle is closed")
}
