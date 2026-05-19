/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::fs::{
    self,
    File,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::{
    Path,
    PathBuf,
};

use log::warn;

use crate::Files;

use super::files::create_temp_file_in_dir;

/// Temporary file that is removed automatically unless kept or persisted.
///
/// `TempFile` owns both the temporary file path and the open file handle. The
/// file is closed before the path is removed, kept, or persisted. Use
/// [`TempFile::keep`] to keep the file at its generated path, or
/// [`TempFile::persist`] to move it to a final path.
///
/// Cleanup performed from `Drop` is best-effort. If removal fails, the failure
/// is reported through the `log` facade at warning level and the program is not
/// panicked.
#[derive(Debug)]
pub struct TempFile {
    path: Option<PathBuf>,
    file: Option<File>,
}

impl TempFile {
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
            Files::DEFAULT_TEMP_FILE_RETRIES,
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

    /// Returns a shared reference to the open file handle.
    ///
    /// # Returns
    /// The open file handle.
    ///
    /// # Errors
    /// Returns [`ErrorKind::NotFound`] when the file has already been closed.
    #[inline]
    pub fn file(&self) -> Result<&File> {
        self.file.as_ref().ok_or_else(file_closed_error)
    }

    /// Returns a mutable reference to the open file handle.
    ///
    /// # Returns
    /// The open file handle.
    ///
    /// # Errors
    /// Returns [`ErrorKind::NotFound`] when the file has already been closed.
    #[inline]
    pub fn file_mut(&mut self) -> Result<&mut File> {
        self.file.as_mut().ok_or_else(file_closed_error)
    }

    /// Closes the temporary file handle while keeping path cleanup active.
    ///
    /// # Errors
    /// This method currently returns no close-time I/O errors because closing a
    /// standard-library [`File`] is performed by dropping the handle.
    #[inline]
    pub fn close(&mut self) -> Result<()> {
        let _ = self.file.take();
        Ok(())
    }

    /// Keeps the temporary file at its generated path.
    ///
    /// This consumes the guard, closes the file handle, and disables automatic
    /// cleanup.
    ///
    /// # Returns
    /// The generated temporary file path.
    #[inline]
    pub fn keep(mut self) -> PathBuf {
        let _ = self.file.take();
        self.path
            .take()
            .expect("temporary file path has already been released")
    }

    /// Moves the temporary file to a final path.
    ///
    /// The file handle is closed before renaming. Parent directories for
    /// `target` are created before renaming. If the rename fails, the temporary
    /// file remains owned by this guard and is cleaned up when the guard is
    /// dropped.
    ///
    /// # Parameters
    /// - `target`: Final file path.
    ///
    /// # Returns
    /// The final file path.
    ///
    /// # Errors
    /// Returns an I/O error when the parent directory cannot be created or the
    /// temporary file cannot be renamed to `target`.
    pub fn persist<P>(mut self, target: P) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.close()?;
        let target = target.as_ref().to_path_buf();
        Files::ensure_parent(&target)?;
        let source = self
            .path
            .as_ref()
            .expect("temporary file path has already been released");
        fs::rename(source, &target)?;
        let _ = self.path.take();
        Ok(target)
    }
}

impl Drop for TempFile {
    /// Closes and removes the temporary file unless ownership has been released.
    fn drop(&mut self) {
        let _ = self.file.take();
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
/// An [`ErrorKind::NotFound`] error describing the closed file handle.
fn file_closed_error() -> Error {
    Error::new(ErrorKind::NotFound, "temporary file handle is closed")
}
