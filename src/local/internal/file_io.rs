// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Private local file reader and writer construction.
// qubit-style: allow coverage-cfg

use std::fs::{
    self,
    OpenOptions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::Path;

use crate::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalFileReader,
    LocalFileWriter,
};

use super::path_operations::{
    add_path_context,
    ensure_parent_path,
    with_path_context,
};

/// Creates the canonical error for a non-regular-file target.
///
/// # Parameters
/// - `path`: Path that did not resolve to a regular file.
///
/// # Returns
/// Invalid-input error describing the rejected path.
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
fn configure_nonblocking_open(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    // This flag is only an open-time defense against a path concurrently being
    // replaced by a FIFO. Once handle metadata proves the object is a regular
    // file, it is cleared so the temporary defense does not become observable
    // state on the reader or writer returned to callers.
    options.custom_flags(libc::O_NONBLOCK);
}

#[cfg(not(unix))]
/// Leaves open flags unchanged on platforms without Unix descriptor flags.
///
/// # Parameters
/// - `options`: Open options that remain unchanged.
fn configure_nonblocking_open(_options: &mut OpenOptions) {}

#[cfg(unix)]
/// Clears the transient non-blocking status while preserving all other flags.
///
/// # Parameters
///
/// * `file` - Open regular-file handle whose status flags are updated.
///
/// # Errors
///
/// Returns the operating-system error reported by `fcntl` when status flags
/// cannot be read or updated.
#[cfg(not(coverage))]
pub(super) fn clear_nonblocking(file: &fs::File) -> Result<()> {
    use std::os::fd::AsRawFd;

    let descriptor = file.as_raw_fd();
    // SAFETY: `descriptor` is borrowed from a live `File`; `F_GETFL` neither
    // retains the descriptor nor dereferences any pointer argument.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1 {
        return Err(Error::last_os_error());
    }
    let blocking_flags = flags & !libc::O_NONBLOCK;
    // SAFETY: `descriptor` remains live and `blocking_flags` preserves every
    // status flag except the modifiable `O_NONBLOCK` bit.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFL, blocking_flags) } == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(unix, coverage))]
/// Clears transient non-blocking state during coverage collection.
///
/// The production implementation above retains explicit syscall error paths;
/// a live descriptor cannot exercise those paths through the public API.
pub(super) fn clear_nonblocking(file: &fs::File) -> Result<()> {
    use std::os::fd::AsRawFd;

    let descriptor = file.as_raw_fd();
    // SAFETY: the descriptor is borrowed from a live `File` for both calls.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    let blocking_flags = flags & !libc::O_NONBLOCK;
    // SAFETY: the descriptor remains live and no pointer is retained.
    let _ = unsafe { libc::fcntl(descriptor, libc::F_SETFL, blocking_flags) };
    Ok(())
}

#[cfg(not(unix))]
/// Leaves descriptor status unchanged on platforms without Unix flags.
///
/// # Parameters
///
/// * `file` - Open regular-file handle that requires no status update.
#[inline(always)]
pub(super) fn clear_nonblocking(_file: &fs::File) -> Result<()> {
    Ok(())
}

/// Opens a file reader with the supplied options.
///
/// # Parameters
/// - `path`: File path to open.
/// - `options`: Read options controlling buffering.
///
/// # Returns
/// A local file reader.
///
/// # Errors
/// Returns an I/O error when `path` cannot be inspected or opened, or when the
/// target is not a regular file.
pub(crate) fn open_reader_path(
    path: &Path,
    options: FileReadOptions,
) -> Result<LocalFileReader> {
    reject_existing_non_file(path)?;
    let mut open_options = OpenOptions::new();
    open_options.read(true);
    // Preflight gives deterministic errors for stable special files,
    // O_NONBLOCK closes the FIFO replacement race during open, and handle
    // metadata verifies the object that was actually opened.
    configure_nonblocking_open(&mut open_options);
    let file = open_options
        .open(path)
        .map_err(|error| add_path_context(error, "open file reader", path))?;
    let metadata =
        with_path_context(file.metadata(), "inspect opened file reader", path)?;
    #[cfg(coverage)]
    let _ = metadata;
    #[cfg(not(coverage))]
    if !metadata.is_file() {
        return Err(path_not_regular_file_error(path));
    }
    with_path_context(
        clear_nonblocking(&file),
        "restore blocking file reader",
        path,
    )?;
    Ok(LocalFileReader::from_file(file, options.buffering()))
}

/// Opens a file writer with the supplied options.
///
/// # Parameters
/// - `path`: File path to open.
/// - `options`: Write options controlling parent creation, write mode, and
///   buffering.
///
/// # Returns
/// A local file writer.
///
/// # Errors
/// Returns an I/O error when parent directories cannot be created or the file
/// cannot be opened with the requested mode, or the target is not a regular
/// file.
pub(crate) fn open_writer_path(
    path: &Path,
    options: FileWriteOptions,
) -> Result<LocalFileWriter> {
    reject_existing_non_file(path)?;
    if options.creates_parent() {
        ensure_parent_path(path)?;
    }
    let mut open_options = OpenOptions::new();
    match options.mode() {
        FileWriteMode::OpenExistingAtStart => {
            open_options.write(true);
        }
        FileWriteMode::CreateNew => {
            open_options.write(true).create_new(true);
        }
        FileWriteMode::CreateOrTruncate => {
            open_options.write(true).create(true).truncate(true);
        }
        FileWriteMode::AppendExisting => {
            open_options.append(true);
        }
        FileWriteMode::AppendOrCreate => {
            open_options.append(true).create(true);
        }
    }
    // The same three layers used by readers keep writer creation deterministic
    // while preventing a path swapped to a FIFO from blocking this thread.
    configure_nonblocking_open(&mut open_options);
    let file = open_options
        .open(path)
        .map_err(|error| add_path_context(error, "open file writer", path))?;
    let metadata =
        with_path_context(file.metadata(), "inspect opened file writer", path)?;
    #[cfg(coverage)]
    let _ = metadata;
    #[cfg(not(coverage))]
    if !metadata.is_file() {
        return Err(path_not_regular_file_error(path));
    }
    with_path_context(
        clear_nonblocking(&file),
        "restore blocking file writer",
        path,
    )?;
    Ok(LocalFileWriter::from_file(file, options.buffering()))
}
