/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::fs;
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

use crate::LocalFiles;

use super::local_files::create_temp_dir_in_dir;

/// Temporary directory that is removed automatically unless kept or persisted.
///
/// `LocalTempDir` owns a directory path and removes that directory tree when the
/// object is dropped. Use [`LocalTempDir::keep`] to keep the temporary directory at
/// its generated path, or [`LocalTempDir::persist`] to move it to a final path.
///
/// Cleanup performed from `Drop` is best-effort. If removal fails, the failure
/// is reported through the `log` facade at warning level and the program is not
/// panicked.
#[derive(Debug)]
pub struct LocalTempDir {
    path: Option<PathBuf>,
}

impl LocalTempDir {
    /// Creates a temporary directory in the process temporary directory.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created or a unique temporary directory cannot be created.
    #[inline]
    pub fn new() -> Result<Self> {
        Self::with_prefix(None)
    }

    /// Creates a temporary directory in the process temporary directory.
    ///
    /// # Parameters
    /// - `prefix`: Optional directory-name prefix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `prefix` is not a safe file-name fragment, or a unique
    /// temporary directory cannot be created.
    #[inline]
    pub fn with_prefix(prefix: Option<&str>) -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            prefix,
            LocalFiles::DEFAULT_TEMP_FILE_RETRIES,
        )
    }

    /// Creates a temporary directory in the specified directory.
    ///
    /// # Parameters
    /// - `dir`: Parent directory in which the temporary directory is created.
    /// - `prefix`: Optional directory-name prefix.
    /// - `max_tries`: Maximum number of random names to try.
    ///
    /// # Errors
    /// Returns an I/O error when `dir` cannot be created, `prefix` is not a
    /// safe file-name fragment, the retry limit is zero, all generated names
    /// collide, or directory creation fails.
    pub fn in_dir<P>(dir: P, prefix: Option<&str>, max_tries: usize) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = create_temp_dir_in_dir(dir.as_ref(), prefix, max_tries)?;
        Ok(Self { path: Some(path) })
    }

    /// Returns the temporary directory path.
    ///
    /// # Returns
    /// Borrowed path managed by this temporary directory.
    #[inline]
    pub fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("temporary directory path has already been released")
    }

    /// Keeps the temporary directory at its generated path.
    ///
    /// This consumes the guard and disables automatic cleanup.
    ///
    /// # Returns
    /// The generated temporary directory path.
    #[inline]
    pub fn keep(mut self) -> PathBuf {
        self.path
            .take()
            .expect("temporary directory path has already been released")
    }

    /// Moves the temporary directory to a final path.
    ///
    /// Parent directories for `target` are created before renaming. If the
    /// rename fails, the temporary directory remains owned by this guard and is
    /// cleaned up when the guard is dropped.
    ///
    /// # Parameters
    /// - `target`: Final directory path.
    ///
    /// # Returns
    /// The final directory path.
    ///
    /// # Errors
    /// Returns an I/O error when the parent directory cannot be created, the
    /// target already exists, or the temporary directory cannot be renamed to
    /// `target`.
    pub fn persist<P>(mut self, target: P) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        let target = target.as_ref().to_path_buf();
        LocalFiles::ensure_parent(&target)?;
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("target already exists: {}", target.display()),
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let source = self
            .path
            .as_ref()
            .expect("temporary directory path has already been released");
        fs::rename(source, &target)?;
        let _ = self.path.take();
        Ok(target)
    }
}

impl Drop for LocalTempDir {
    /// Removes the temporary directory unless ownership has been released.
    fn drop(&mut self) {
        if let Some(path) = self.path.take()
            && let Err(error) = fs::remove_dir_all(&path)
        {
            warn!(
                "failed to remove temporary directory {}: {}",
                path.display(),
                error
            );
        }
    }
}
