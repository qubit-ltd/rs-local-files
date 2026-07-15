// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private temporary-entry creation.

use std::fs::{
    DirBuilder,
    File,
    OpenOptions,
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

#[cfg(unix)]
use std::os::unix::fs::{
    DirBuilderExt,
    OpenOptionsExt,
};

use crate::LocalFilenames;

use super::path_operations::{
    add_path_context,
    ensure_dir_path,
};

/// Default number of attempts used when creating a random temporary entry.
pub(crate) const DEFAULT_TEMP_FILE_RETRIES: usize = 256;

/// Creates a unique temporary file in `dir`.
///
/// # Parameters
/// - `dir`: Directory in which to create the file.
/// - `prefix`: Optional file-name prefix.
/// - `suffix`: Optional file-name suffix.
/// - `max_tries`: Maximum number of generated names to try.
///
/// # Returns
/// The created temporary path and open file handle.
///
/// # Errors
/// Returns an I/O error when `dir` cannot be created, `max_tries` is zero, all
/// generated names collide, or file creation fails.
pub(crate) fn create_temp_file_in_dir(
    dir: &Path,
    prefix: Option<&str>,
    suffix: Option<&str>,
    max_tries: usize,
) -> Result<(PathBuf, File)> {
    validate_max_tries(max_tries)?;
    ensure_dir_path(dir)?;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let path = dir.join(LocalFilenames::try_random_with(prefix, suffix)?);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) => {
                if error.kind() == ErrorKind::AlreadyExists
                    && attempt < max_tries
                {
                    continue;
                }
                return Err(add_path_context(
                    error,
                    "create temporary file",
                    &path,
                ));
            }
        }
    }
}

/// Creates a unique temporary directory in `dir`.
///
/// # Parameters
/// - `dir`: Directory in which to create the directory.
/// - `prefix`: Optional directory-name prefix.
/// - `max_tries`: Maximum number of generated names to try.
///
/// # Returns
/// The created temporary directory path.
///
/// # Errors
/// Returns an I/O error when `dir` cannot be created, `max_tries` is zero, all
/// generated names collide, or directory creation fails.
pub(crate) fn create_temp_dir_in_dir(
    dir: &Path,
    prefix: Option<&str>,
    max_tries: usize,
) -> Result<PathBuf> {
    validate_max_tries(max_tries)?;
    ensure_dir_path(dir)?;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let path = dir.join(LocalFilenames::try_random_with(prefix, None)?);
        match create_private_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) => {
                if error.kind() == ErrorKind::AlreadyExists
                    && attempt < max_tries
                {
                    continue;
                }
                return Err(add_path_context(
                    error,
                    "create temporary directory",
                    &path,
                ));
            }
        }
    }
}

/// Creates a directory with private permissions where the platform supports
/// explicit modes.
///
/// On Unix the directory is created with mode `0o700`, subject only to a more
/// restrictive process umask. Other platforms use their native inherited
/// access-control behavior.
///
/// # Parameters
/// - `path`: Directory path to create.
///
/// # Errors
/// Returns the I/O error reported while creating the directory.
pub(crate) fn create_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    let mut builder = DirBuilder::new();
    #[cfg(not(unix))]
    let builder = DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)
}

/// Validates a retry count.
///
/// # Parameters
/// - `max_tries`: Retry count to validate.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `max_tries` is zero.
fn validate_max_tries(max_tries: usize) -> Result<()> {
    if max_tries == 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "temporary entry retry count must be greater than zero",
        ));
    }
    Ok(())
}
