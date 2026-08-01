// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private local file reader and writer construction.
// qubit-style: allow coverage-cfg
// Public APIs cannot force an opened regular-file metadata failure.

use std::fs::{
    self,
    OpenOptions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

use crate::{
    read,
    write,
};

use super::io_result_context::with_path_context;
use super::path_operations::{
    add_path_context,
    ensure_parent_path,
};
#[cfg(unix)]
use super::unix_nonblocking::{
    clear_nonblocking,
    open_with_nonblocking_retry,
};

/// Creates the canonical error for a non-regular-file target.
///
/// # Parameters
///
/// * `path` - The path rendered in the error message.
///
/// # Returns
///
/// An `InvalidInput` error identifying the non-regular path.
#[must_use]
#[inline]
fn path_not_regular_file_error(path: &Path) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("path is not a regular file: {}", path.display()),
    )
}

/// Rejects an existing path unless it resolves to a regular file.
///
/// Missing paths remain valid because writer modes may create them and reader
/// construction should preserve the filesystem's original not-found error.
///
/// # Parameters
/// - `path`: Path to inspect before opening.
///
/// # Errors
/// Returns `InvalidInput` when the existing path is not a regular file, or the
/// contextualized inspection error when metadata cannot be read.
fn reject_existing_non_file(path: &Path) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(path_not_regular_file_error(path)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(add_path_context(error, "inspect file", path)),
    }
}

#[cfg(unix)]
/// Adds a non-blocking open flag to prevent a concurrent FIFO replacement from
/// hanging the opening thread.
///
/// # Parameters
/// - `options`: Open options to update.
#[inline(always)]
fn configure_nonblocking_open(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    // This flag is only an open-time defense against a path concurrently being
    // replaced by a FIFO. Once handle metadata proves the object is a regular
    // file, it is cleared so the temporary defense does not become observable
    // state on the reader or writer returned to callers.
    options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
}

#[cfg(not(unix))]
/// Leaves open flags unchanged on platforms without Unix descriptor flags.
///
/// # Parameters
/// - `options`: Open options that remain unchanged.
#[inline(always)]
fn configure_nonblocking_open(_options: &mut OpenOptions) {}

/// Opens a path with ordinary blocking semantics after applying safety flags.
///
/// # Parameters
///
/// * `options` - Configured native open options.
/// * `path` - Path opened by the native operation.
///
/// # Returns
///
/// The opened file handle.
///
/// # Errors
///
/// Returns the native open error. On Unix, lease conflicts are retried to
/// preserve ordinary blocking-open behavior.
#[inline]
fn open_configured_file(
    options: &OpenOptions,
    path: &Path,
    open_retry_timeout: Option<Duration>,
) -> Result<fs::File> {
    #[cfg(unix)]
    {
        open_with_nonblocking_retry(open_retry_timeout, || options.open(path))
    }
    #[cfg(not(unix))]
    {
        let _ = open_retry_timeout;
        options.open(path)
    }
}

/// Clears the transient non-blocking status after handle validation.
///
/// # Parameters
///
/// * `file` - Open file handle whose status is normalized.
///
/// # Returns
///
/// `Ok(())` after the handle has ordinary blocking behavior.
///
/// # Errors
///
/// Returns the native descriptor-status error on Unix.
#[inline(always)]
fn clear_transient_nonblocking(file: &fs::File) -> Result<()> {
    #[cfg(unix)]
    {
        clear_nonblocking(file.as_raw_fd())
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

/// Verifies an opened handle and restores ordinary blocking behavior.
///
/// # Parameters
///
/// * `file` - The opened handle to inspect and normalize.
/// * `inspect_operation` - The operation label used for metadata errors.
/// * `restore_operation` - The operation label used for descriptor errors.
/// * `path` - The diagnostic path attached to failures.
///
/// # Returns
///
/// `Ok(())` after a regular file handle has been verified and normalized to
/// blocking mode.
///
/// # Errors
///
/// Returns a contextual metadata or descriptor error, or `InvalidInput` when a
/// racing path replacement produced a non-regular handle.
fn prepare_opened_regular_file(
    file: &fs::File,
    inspect_operation: &'static str,
    restore_operation: &'static str,
    path: &Path,
) -> Result<()> {
    let metadata_result = file.metadata();
    #[cfg(coverage)]
    let metadata_result =
        if super::coverage_fault::is_enabled("file-handle-metadata") {
            Err(Error::other("injected opened-file metadata failure"))
        } else {
            metadata_result
        };
    let metadata = with_path_context(metadata_result, inspect_operation, path)?;
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("file-handle-type") {
        return Err(path_not_regular_file_error(path));
    }
    if !metadata.is_file() {
        return Err(path_not_regular_file_error(path));
    }
    with_path_context(
        clear_transient_nonblocking(file),
        restore_operation,
        path,
    )
}

/// Opens and validates one unbuffered regular file for reading.
///
/// # Parameters
/// - `path`: File path to inspect and open.
/// - `open_retry_timeout`: Optional Unix lease-conflict retry timeout.
///
/// # Returns
/// The validated standard-library file handle.
///
/// # Errors
/// Returns a contextual I/O error when the path cannot be inspected or opened,
/// or when the opened object is not a regular file.
fn open_reader_file(
    path: &Path,
    open_retry_timeout: Option<Duration>,
) -> Result<fs::File> {
    reject_existing_non_file(path)?;
    let mut open_options = OpenOptions::new();
    open_options.read(true);
    configure_nonblocking_open(&mut open_options);
    let file = open_configured_file(&open_options, path, open_retry_timeout)
        .map_err(|error| add_path_context(error, "open file reader", path))?;
    prepare_opened_regular_file(
        &file,
        "inspect opened file reader",
        "restore blocking file reader",
        path,
    )?;
    Ok(file)
}

/// Opens an unbuffered regular file through the native read API.
///
/// # Parameters
/// - `path`: File path to inspect and open.
/// - `options`: Native read-open options.
///
/// # Returns
/// The validated standard-library file handle.
///
/// # Errors
/// Returns a contextual I/O error when the path cannot be inspected or opened,
/// or when the opened object is not a regular file.
#[inline(always)]
pub(crate) fn open_native_reader_path(
    path: &Path,
    options: &read::OpenOptions,
) -> Result<fs::File> {
    open_reader_file(path, options.open_retry_timeout())
}

/// Opens and validates one unbuffered regular file for writing.
///
/// # Parameters
/// - `path`: File path to inspect and open.
/// - `create_parent`: Whether missing parent directories are created.
/// - `mode`: Native file creation and positioning mode.
/// - `open_retry_timeout`: Optional Unix lease-conflict retry timeout.
///
/// # Returns
/// The validated standard-library file handle.
///
/// # Errors
/// Returns a contextual I/O error when parent creation, inspection, opening, or
/// post-open truncation fails.
fn open_writer_file(
    path: &Path,
    create_parent: bool,
    mode: write::Mode,
    open_retry_timeout: Option<Duration>,
) -> Result<fs::File> {
    reject_existing_non_file(path)?;
    if create_parent {
        ensure_parent_path(path)?;
    }
    let should_truncate = mode == write::Mode::CreateOrTruncate;
    let mut open_options = OpenOptions::new();
    match mode {
        write::Mode::OpenExistingAtStart => {
            open_options.write(true);
        }
        write::Mode::CreateNew => {
            open_options.write(true).create_new(true);
        }
        write::Mode::CreateOrTruncate => {
            open_options.write(true).create(true);
        }
        write::Mode::AppendExisting => {
            open_options.append(true);
        }
        write::Mode::AppendOrCreate => {
            open_options.append(true).create(true);
        }
    }
    configure_nonblocking_open(&mut open_options);
    let file = open_configured_file(&open_options, path, open_retry_timeout)
        .map_err(|error| add_path_context(error, "open file writer", path))?;
    prepare_opened_regular_file(
        &file,
        "inspect opened file writer",
        "restore blocking file writer",
        path,
    )?;
    if should_truncate {
        with_path_context(
            file.set_len(0),
            "truncate opened file writer",
            path,
        )?;
    }
    Ok(file)
}

/// Opens an unbuffered regular file through the native write API.
///
/// # Parameters
/// - `path`: File path to inspect and open.
/// - `options`: Native write-open options.
///
/// # Returns
/// The validated standard-library file handle.
///
/// # Errors
/// Returns a contextual I/O error when parent creation, inspection, opening, or
/// post-open truncation fails.
#[inline]
pub(crate) fn open_native_writer_path(
    path: &Path,
    options: &write::OpenOptions,
) -> Result<fs::File> {
    open_writer_file(
        path,
        options.creates_parents(),
        options.mode(),
        options.open_retry_timeout(),
    )
}
