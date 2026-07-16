// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Unix descriptor-relative rooted ordinary-file operations.
// qubit-style: allow coverage-cfg

use std::ffi::{
    CString,
    OsStr,
};
use std::fs::{
    File,
    OpenOptions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::os::fd::{
    AsRawFd,
    FromRawFd,
};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{
    Path,
    PathBuf,
};

use crate::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalFileReader,
    LocalFileWriter,
    LocalRelativePath,
};

use super::file_io::clear_nonblocking;
use super::path_operations::{
    add_path_context,
    with_path_context,
};

/// Opens a no-follow directory handle for a root path.
///
/// # Parameters
///
/// * `path` - Absolute diagnostic path to open and anchor.
///
/// # Returns
///
/// An open directory descriptor used as rooted authority.
///
/// # Errors
///
/// Returns a contextual I/O error when the path is missing, linked, not a
/// directory, or cannot be opened.
pub(crate) fn open_root_directory(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let directory =
        rooted_open_result(options.open(path), "open root directory", path)?;
    let metadata = with_path_context(
        directory.metadata(),
        "inspect root directory",
        path,
    )?;
    #[cfg(coverage)]
    let _ = metadata;
    #[cfg(not(coverage))]
    if !metadata.is_dir() {
        return Err(rooted_type_error(path, "directory"));
    }
    Ok(directory)
}

/// Opens an ordinary reader relative to an anchored root descriptor.
///
/// # Parameters
///
/// * `root` - Open root directory authority.
/// * `diagnostic_root` - Path used only to contextualize errors.
/// * `path` - Validated relative target path.
/// * `options` - Buffering options for the returned reader.
///
/// # Returns
///
/// A reader for the opened regular file.
///
/// # Errors
///
/// Returns a contextual error for traversal, symbolic-link, resource-type,
/// open, metadata, or descriptor-status failures.
pub(crate) fn open_rooted_reader(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    options: FileReadOptions,
) -> Result<LocalFileReader> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name) =
        open_rooted_parent(root, &diagnostic_path, path, false)?;
    reject_existing_non_file(&parent, &name, &diagnostic_path)?;
    let flags =
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    let file = rooted_open_result(
        open_file_at(&parent, &name, flags, 0),
        "open rooted file reader",
        &diagnostic_path,
    )?;
    verify_regular_file(&file, &diagnostic_path)?;
    with_path_context(
        clear_nonblocking(&file),
        "restore blocking rooted file reader",
        &diagnostic_path,
    )?;
    Ok(LocalFileReader::from_file(file, options.buffering()))
}

/// Opens an ordinary writer relative to an anchored root descriptor.
///
/// # Parameters
///
/// * `root` - Open root directory authority.
/// * `diagnostic_root` - Path used only to contextualize errors.
/// * `path` - Validated relative target path.
/// * `options` - Parent, mode, and buffering options.
///
/// # Returns
///
/// A writer for the opened regular file.
///
/// # Errors
///
/// Returns a contextual error for traversal, parent creation, symbolic-link,
/// resource-type, open, metadata, or descriptor-status failures.
pub(crate) fn open_rooted_writer(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    options: FileWriteOptions,
) -> Result<LocalFileWriter> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name) = open_rooted_parent(
        root,
        &diagnostic_path,
        path,
        options.creates_parent(),
    )?;
    reject_existing_non_file(&parent, &name, &diagnostic_path)?;
    let mut flags = libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    match options.mode() {
        FileWriteMode::OpenExistingAtStart => flags |= libc::O_WRONLY,
        FileWriteMode::CreateNew => {
            flags |= libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL;
        }
        FileWriteMode::CreateOrTruncate => {
            flags |= libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
        }
        FileWriteMode::AppendExisting => {
            flags |= libc::O_WRONLY | libc::O_APPEND;
        }
        FileWriteMode::AppendOrCreate => {
            flags |= libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT;
        }
    }
    let file = rooted_open_result(
        open_file_at(&parent, &name, flags, 0o600),
        "open rooted file writer",
        &diagnostic_path,
    )?;
    verify_regular_file(&file, &diagnostic_path)?;
    with_path_context(
        clear_nonblocking(&file),
        "restore blocking rooted file writer",
        &diagnostic_path,
    )?;
    Ok(LocalFileWriter::from_file(file, options.buffering()))
}

/// Opens the destination parent by traversing only from `root`.
///
/// # Parameters
///
/// * `root` - Open root directory authority.
/// * `diagnostic_path` - Full diagnostic target path.
/// * `path` - Validated relative target path.
/// * `create` - Whether missing parent directories should be created.
///
/// # Returns
///
/// The open parent directory and final entry name.
///
/// # Errors
///
/// Returns a contextual error when a parent cannot be cloned, created, opened
/// without following a link, or verified as a directory.
pub(in crate::local) fn open_rooted_parent(
    root: &File,
    diagnostic_path: &Path,
    path: &LocalRelativePath,
    create: bool,
) -> Result<(File, CString)> {
    let final_name = path
        .as_path()
        .file_name()
        .expect("validated relative paths always have a final component");
    let final_name = component_c_string(final_name);
    let mut directory = with_path_context(
        root.try_clone(),
        "clone root directory",
        diagnostic_path,
    )?;
    let parent = path.as_path().parent().unwrap_or(Path::new(""));
    let mut diagnostic_root = diagnostic_path.to_path_buf();
    for _ in path.as_path().components() {
        let _ = diagnostic_root.pop();
    }
    let mut traversed = PathBuf::new();
    for component in parent.components() {
        let name = component.as_os_str();
        traversed.push(name);
        let component_path = diagnostic_root.join(&traversed);
        directory = match open_directory_at(&directory, name) {
            Ok(next) => next,
            Err(error) if create && error.kind() == ErrorKind::NotFound => {
                create_directory_at(&directory, name, &component_path)?;
                rooted_open_result(
                    open_directory_at(&directory, name),
                    "open created rooted directory",
                    &component_path,
                )?
            }
            Err(error) => {
                return Err(rooted_open_error(
                    error,
                    "open rooted directory component",
                    &component_path,
                ));
            }
        };
        let metadata = with_path_context(
            directory.metadata(),
            "inspect rooted directory component",
            &component_path,
        )?;
        #[cfg(coverage)]
        let _ = metadata;
        #[cfg(not(coverage))]
        if !metadata.is_dir() {
            return Err(rooted_type_error(&component_path, "directory"));
        }
    }
    Ok((directory, final_name))
}

/// Opens one no-follow directory entry relative to `parent`.
///
/// # Parameters
///
/// * `parent` - Open parent directory descriptor.
/// * `name` - Single child component to open.
///
/// # Returns
///
/// An owned child directory handle.
///
/// # Errors
///
/// Returns the operating-system error from component conversion or `openat`.
fn open_directory_at(parent: &File, name: &OsStr) -> Result<File> {
    let name = component_c_string(name);
    open_file_at(
        parent,
        &name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        0,
    )
}

/// Creates one private directory entry relative to `parent`.
///
/// # Parameters
///
/// * `parent` - Open parent directory descriptor.
/// * `name` - Single child component to create.
/// * `diagnostic_path` - Path used only for error context.
///
/// # Errors
///
/// Returns a contextual error when component conversion or `mkdirat` fails.
fn create_directory_at(
    parent: &File,
    name: &OsStr,
    diagnostic_path: &Path,
) -> Result<()> {
    #[cfg(coverage)]
    let _ = diagnostic_path;
    let name = component_c_string(name);
    // SAFETY: the parent descriptor and NUL-terminated component remain live;
    // `mkdirat` does not retain either value.
    let result =
        unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    #[cfg(coverage)]
    let _ = result;
    #[cfg(not(coverage))]
    if result == -1 {
        let error = Error::last_os_error();
        if error.kind() != ErrorKind::AlreadyExists {
            return Err(add_path_context(
                error,
                "create rooted directory",
                diagnostic_path,
            ));
        }
    }
    Ok(())
}

/// Opens one entry relative to an open directory descriptor.
///
/// # Parameters
///
/// * `parent` - Open parent directory descriptor.
/// * `name` - NUL-terminated single entry name.
/// * `flags` - Native `openat` flags.
/// * `mode` - Creation permissions used when `O_CREAT` is present.
///
/// # Returns
///
/// An owned file handle for the opened entry.
///
/// # Errors
///
/// Returns the operating-system error reported by `openat`.
pub(super) fn open_file_at(
    parent: &File,
    name: &CString,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<File> {
    // SAFETY: the parent descriptor and component string remain live for the
    // call. A successful descriptor is transferred immediately into `File`.
    // Variadic integer arguments require default C promotion. This cast is a
    // no-op on Linux and promotes narrower `mode_t` definitions on macOS.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags,
            libc::c_uint::from(mode),
        )
    };
    if descriptor == -1 {
        return Err(Error::last_os_error());
    }
    // SAFETY: `openat` returned a new owned descriptor that has not been
    // wrapped elsewhere.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

/// Rejects an existing final entry unless it is a regular file.
///
/// # Parameters
///
/// * `parent` - Open destination parent descriptor.
/// * `name` - Final entry name.
/// * `diagnostic_path` - Path used only for error context.
///
/// # Errors
///
/// Returns `InvalidInput` for stable links and non-files, or a contextual
/// inspection error other than `NotFound`.
fn reject_existing_non_file(
    parent: &File,
    name: &CString,
    diagnostic_path: &Path,
) -> Result<()> {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `status` points to writable storage, and the live parent and name
    // values remain valid for this non-retaining call.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        #[cfg(coverage)]
        return Ok(());
        #[cfg(not(coverage))]
        {
            let error = Error::last_os_error();
            return if error.kind() == ErrorKind::NotFound {
                Ok(())
            } else {
                Err(add_path_context(
                    error,
                    "inspect rooted file entry",
                    diagnostic_path,
                ))
            };
        }
    }
    // SAFETY: successful `fstatat` initialized the complete `stat` value.
    let status = unsafe { status.assume_init() };
    if status.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(rooted_type_error(diagnostic_path, "regular file"));
    }
    Ok(())
}

/// Verifies that an opened handle is a regular file.
///
/// # Parameters
///
/// * `file` - Handle opened by a rooted final operation.
/// * `diagnostic_path` - Path used only for error context.
///
/// # Errors
///
/// Returns a contextual metadata error or `InvalidInput` for a non-file.
fn verify_regular_file(file: &File, diagnostic_path: &Path) -> Result<()> {
    let metadata = with_path_context(
        file.metadata(),
        "inspect rooted file handle",
        diagnostic_path,
    )?;
    #[cfg(coverage)]
    let _ = metadata;
    #[cfg(not(coverage))]
    if !metadata.is_file() {
        return Err(rooted_type_error(diagnostic_path, "regular file"));
    }
    Ok(())
}

/// Converts a validated normal component to a native C string.
///
/// # Parameters
///
/// * `component` - Single validated path component.
///
/// # Returns
///
/// A NUL-terminated native component.
///
/// # Errors
///
/// Returns `InvalidInput` if the component unexpectedly contains NUL.
fn component_c_string(component: &OsStr) -> CString {
    CString::new(component.as_bytes())
        .expect("LocalRelativePath guarantees components without NUL")
}

/// Adds rooted-open normalization and path context to an open result.
fn rooted_open_result<T>(
    result: Result<T>,
    operation: &'static str,
    path: &Path,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(rooted_open_error(error, operation, path)),
    }
}

/// Normalizes link and wrong-directory open failures to `InvalidInput`.
///
/// # Parameters
///
/// * `error` - Native open error to contextualize.
/// * `operation` - Human-readable operation description.
/// * `path` - Diagnostic path associated with the operation.
///
/// # Returns
///
/// A contextual error preserving ordinary kinds and making link denial
/// deterministic across Unix implementations.
fn rooted_open_error(
    error: Error,
    operation: &'static str,
    path: &Path,
) -> Error {
    let error = match error.raw_os_error() {
        Some(code) if code == libc::ELOOP || code == libc::ENOTDIR => {
            Error::new(ErrorKind::InvalidInput, error)
        }
        _ => error,
    };
    add_path_context(error, operation, path)
}

/// Creates the canonical error for a rooted resource of the wrong type.
///
/// # Parameters
///
/// * `path` - Diagnostic path for the rejected entry.
/// * `expected` - Expected resource type description.
///
/// # Returns
///
/// An invalid-input error naming the required type.
fn rooted_type_error(path: &Path, expected: &str) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("rooted path is not a {expected}: {}", path.display(),),
    )
}
