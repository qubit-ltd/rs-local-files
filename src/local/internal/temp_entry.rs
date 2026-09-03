// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private temporary-entry creation.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs::DirBuilder;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use super::file_name_validation::validate_file_name_fragment;
use super::path_operations::add_path_context;
#[cfg(feature = "test-support")]
use crate::local::internal::test_support;
use crate::local::try_random_file_name;

/// Validates caller-provided temporary-entry affixes before sandbox creation.
pub(crate) fn validate_temp_affixes(prefix: Option<&str>, suffix: Option<&str>) -> Result<()> {
    if let Some(prefix) = prefix {
        validate_file_name_fragment("prefix", prefix)?;
    }
    if let Some(suffix) = suffix {
        validate_file_name_fragment("suffix", suffix)?;
    }
    Ok(())
}

/// Creates a unique temporary file in `dir`.
///
/// # Parameters
/// - `dir`: Directory in which to create the file.
/// - `prefix`: Optional file-name prefix.
/// - `suffix`: Optional file-name suffix.
/// - `max_tries`: Optional maximum number of generated names to try.
///
/// # Returns
/// The created temporary path and open file handle.
///
/// # Errors
/// Returns an I/O error when `dir` does not exist, an explicit
/// `max_tries` is zero, all authorized names collide, or file creation fails.
pub(crate) fn create_temp_file_in_dir(
    dir: &Path,
    prefix: Option<&str>,
    suffix: Option<&str>,
    max_tries: Option<usize>,
) -> Result<(PathBuf, File)> {
    validate_max_tries(max_tries)?;
    let mut attempt = 0_usize;
    loop {
        attempt = attempt.saturating_add(1);
        let path = dir.join(try_random_file_name("qubit-local-files-", prefix, suffix)?);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        #[cfg(feature = "test-support")]
        let opened = if test_support::take("temp-file-collision") {
            Err(Error::new(
                ErrorKind::AlreadyExists,
                "injected temporary file collision",
            ))
        } else if test_support::is_enabled("temp-file-open") {
            Err(Error::other("injected temporary file creation failure"))
        } else {
            options.open(&path)
        };
        #[cfg(not(feature = "test-support"))]
        let opened = options.open(&path);
        match opened {
            Ok(file) => return Ok((path, file)),
            Err(error) => {
                if should_retry_collision(&error, attempt, max_tries) {
                    continue;
                }
                return Err(add_path_context(error, "create temporary file", &path));
            }
        }
    }
}

/// Creates a unique temporary directory with validated prefix and suffix.
///
/// # Parameters
///
/// - `dir`: Parent directory.
/// - `prefix`: Optional generated-name prefix.
/// - `suffix`: Optional generated-name suffix.
/// - `max_tries`: Optional maximum collision attempts.
///
/// # Errors
///
/// Returns an I/O error when affix validation, parent lookup, name
/// generation, or directory creation fails.
pub(crate) fn create_temp_dir_in_dir_with_affixes(
    dir: &Path,
    prefix: Option<&str>,
    suffix: Option<&str>,
    max_tries: Option<usize>,
) -> Result<PathBuf> {
    validate_max_tries(max_tries)?;
    let mut attempt = 0_usize;
    loop {
        attempt = attempt.saturating_add(1);
        let path = dir.join(try_random_file_name("qubit-local-files-", prefix, suffix)?);
        #[cfg(feature = "test-support")]
        let created = if test_support::take("temp-directory-collision") {
            Err(Error::new(
                ErrorKind::AlreadyExists,
                "injected temporary directory collision",
            ))
        } else if test_support::is_enabled("temp-directory-create") {
            Err(Error::other("injected temporary directory creation failure"))
        } else {
            create_private_dir(&path)
        };
        #[cfg(not(feature = "test-support"))]
        let created = create_private_dir(&path);
        match created {
            Ok(()) => return Ok(path),
            Err(error) => {
                if should_retry_collision(&error, attempt, max_tries) {
                    continue;
                }
                return Err(add_path_context(error, "create temporary directory", &path));
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
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn create_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    let mut builder = DirBuilder::new();
    #[cfg(not(unix))]
    let builder = DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)
}

/// Tests whether a colliding temporary-entry name should be retried.
///
/// # Parameters
/// - `error`: Entry-creation error.
/// - `attempt`: One-based attempt number that just failed.
/// - `max_tries`: Maximum number of allowed attempts.
///
/// # Returns
/// `true` only for an existing entry when another attempt remains.
#[must_use]
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
fn should_retry_collision(error: &Error, attempt: usize, max_tries: Option<usize>) -> bool {
    error.kind() == ErrorKind::AlreadyExists && max_tries.is_none_or(|max_tries| attempt < max_tries)
}

/// Validates a retry count.
///
/// # Parameters
/// - `max_tries`: Retry count to validate.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `max_tries` is zero.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn validate_max_tries(max_tries: Option<usize>) -> Result<()> {
    if max_tries == Some(0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "temporary entry retry count must be greater than zero",
        ));
    }
    Ok(())
}
