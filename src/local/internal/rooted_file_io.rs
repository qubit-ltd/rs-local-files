// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix descriptor-relative rooted ordinary-file operations.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

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
    LocalRelativePath,
    read,
    write,
};

use super::io_result_context::with_path_context;
use super::path_operations::add_path_context;
use super::rooted_io_result::{
    missing_rooted_entry,
    normalize_mkdirat_result,
    normalize_opened_directory_metadata,
    normalize_opened_regular_file_metadata,
};
use super::rooted_parent::RootedParent;
use super::rooted_parent_mode::RootedParentMode;
use super::unix_nonblocking::{
    clear_nonblocking,
    open_with_nonblocking_retry,
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
#[inline]
pub(crate) fn open_root_directory(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let directory =
        rooted_open_result(options.open(path), "open root directory", path)?;
    verify_opened_directory(&directory, "inspect root directory", path)
        .map(|()| directory)
}

/// Returns the current native path of an opened root for symlink resolution.
///
/// The path is derived from the retained descriptor rather than from the
/// caller's diagnostic path, so it continues to identify the same directory
/// after that path is renamed.
///
/// # Errors
///
/// Returns an I/O error when the operating system cannot recover the path from
/// the retained descriptor. The caller's diagnostic path is never used as a
/// fallback.
pub(crate) fn root_authority_path(root: &File) -> Result<PathBuf> {
    if let Some(error) = crate::local::test_io_error("root-authority-path") {
        return Err(error);
    }
    #[cfg(target_os = "macos")]
    {
        let mut buffer = [0_u8; libc::PATH_MAX as usize];
        // SAFETY: `buffer` is writable storage of the size required by
        // `F_GETPATH`, and `root` owns a live descriptor.
        let result = unsafe {
            libc::fcntl(root.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr())
        };
        if result == 0 {
            let length = buffer
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(buffer.len());
            return Ok(PathBuf::from(OsStr::from_bytes(&buffer[..length])));
        }
        return Err(Error::last_os_error());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let descriptor_path =
            PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()));
        with_path_context(
            std::fs::read_link(&descriptor_path),
            "resolve root authority path",
            &descriptor_path,
        )
    }
}

/// Reads metadata for a final rooted entry without following a symbolic link.
///
/// The traversal opens every parent through directory descriptors before using
/// `fstatat` with `AT_SYMLINK_NOFOLLOW` for the final entry. It therefore
/// preserves the rooted authority while observing directories, links, and
/// special files without opening them as regular files.
///
/// # Parameters
///
/// * `root` - Open directory descriptor that authorizes the lookup.
/// * `diagnostic_root` - Path used only to contextualize I/O errors.
/// * `path` - Validated non-empty relative entry path.
///
/// # Returns
///
/// The initialized native `stat` structure for the final entry.
///
/// # Errors
///
/// Returns a contextual I/O error when a parent cannot be securely traversed
/// or when the final entry cannot be inspected.
pub(crate) fn read_rooted_symlink_metadata(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<libc::stat> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _parent_dirs_to_sync) = open_rooted_parent(
        root,
        &diagnostic_path,
        path,
        RootedParentMode::OpenExisting,
    )?
    .into_parts();
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the live parent descriptor and final component remain valid for
    // the duration of this non-retaining call, and `status` provides writable
    // storage for the complete kernel result.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        return Err(add_path_context(
            Error::last_os_error(),
            "inspect rooted entry",
            &diagnostic_path,
        ));
    }
    // SAFETY: a successful `fstatat` fully initializes `status`.
    Ok(unsafe { status.assume_init() })
}

/// Opens a native file reader relative to an anchored root descriptor.
pub(crate) fn open_rooted_native_reader(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    options: &read::OpenOptions,
) -> Result<File> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _parent_dirs_to_sync) = open_rooted_parent(
        root,
        &diagnostic_path,
        path,
        RootedParentMode::OpenExisting,
    )?
    .into_parts();
    reject_existing_non_file(&parent, &name, &diagnostic_path)?;
    let flags =
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    let file = rooted_open_result(
        open_with_nonblocking_retry(options.open_retry_timeout(), || {
            open_file_at(&parent, &name, flags, 0)
        }),
        "open rooted native file reader",
        &diagnostic_path,
    )?;
    prepare_opened_rooted_regular_file(
        &file,
        "restore blocking rooted native file reader",
        &diagnostic_path,
    )?;
    Ok(file)
}

/// Opens a native file writer relative to an anchored root descriptor.
pub(crate) fn open_rooted_native_writer(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    options: &write::OpenOptions,
) -> Result<File> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let parent_mode = if options.creates_parents() {
        RootedParentMode::CreateMissing
    } else {
        RootedParentMode::OpenExisting
    };
    let (parent, name, _parent_dirs_to_sync) =
        open_rooted_parent(root, &diagnostic_path, path, parent_mode)?
            .into_parts();
    reject_existing_non_file(&parent, &name, &diagnostic_path)?;
    let mode = options.mode();
    let should_truncate = mode == write::Mode::CreateOrTruncate;
    let mut flags = libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    match mode {
        write::Mode::OpenExistingAtStart => flags |= libc::O_WRONLY,
        write::Mode::CreateNew => {
            flags |= libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL
        }
        write::Mode::CreateOrTruncate => {
            flags |= libc::O_WRONLY | libc::O_CREAT
        }
        write::Mode::AppendExisting => flags |= libc::O_WRONLY | libc::O_APPEND,
        write::Mode::AppendOrCreate => {
            flags |= libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT;
        }
    }
    let file = rooted_open_result(
        open_with_nonblocking_retry(options.open_retry_timeout(), || {
            open_file_at(&parent, &name, flags, 0o600)
        }),
        "open rooted native file writer",
        &diagnostic_path,
    )?;
    prepare_opened_rooted_regular_file(
        &file,
        "restore blocking rooted native file writer",
        &diagnostic_path,
    )?;
    if should_truncate {
        with_path_context(
            file.set_len(0),
            "truncate opened rooted native file writer",
            &diagnostic_path,
        )?;
    }
    Ok(file)
}

/// Synchronizes the parent directory of a rooted descendant.
///
/// The parent is reopened solely through the root descriptor, so the
/// synchronization remains within the established rooted authority.
///
/// # Errors
///
/// Returns an error when the parent cannot be opened securely or synchronized.
pub(crate) fn sync_rooted_parent(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<()> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, _name, _parent_dirs_to_sync) = open_rooted_parent(
        root,
        &diagnostic_path,
        path,
        RootedParentMode::OpenExisting,
    )?
    .into_parts();
    with_path_context(
        parent.sync_all(),
        "synchronize rooted parent directory",
        &diagnostic_path,
    )
}

/// Opens the destination parent by traversing only from `root`.
///
/// # Parameters
///
/// * `root` - Open root directory authority.
/// * `diagnostic_path` - Full diagnostic target path.
/// * `path` - Validated relative target path.
/// * `mode` - Missing-parent creation and durability-tracking behavior.
///
/// # Returns
///
/// The open parent directory, final entry name, and ancestor descriptors whose
/// newly created child entries require synchronization.
///
/// # Errors
///
/// Returns a contextual error when a parent cannot be cloned, created, opened
/// without following a link, or verified as a directory.
///
/// # Panics
///
/// Panics if `path` violates the validated relative-path invariant and has no
/// final component.
pub(in crate::local) fn open_rooted_parent(
    root: &File,
    diagnostic_path: &Path,
    path: &LocalRelativePath,
    mode: RootedParentMode,
) -> Result<RootedParent> {
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
    let mut parent_dirs_to_sync = Vec::new();
    for component in parent.components() {
        let name = component.as_os_str();
        traversed.push(name);
        let component_path = diagnostic_root.join(&traversed);
        directory = match open_directory_at(&directory, name) {
            Ok(next) => next,
            Err(error)
                if mode.creates_missing()
                    && error.kind() == ErrorKind::NotFound =>
            {
                let parent_to_sync = if mode.tracks_sync() {
                    Some(with_path_context(
                        directory.try_clone(),
                        "clone rooted parent for synchronization",
                        &component_path,
                    )?)
                } else {
                    None
                };
                create_directory_at(&directory, name, &component_path)?;
                parent_dirs_to_sync.extend(parent_to_sync);
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
        verify_opened_directory(
            &directory,
            "inspect rooted directory component",
            &component_path,
        )?;
    }
    Ok(RootedParent::new(
        directory,
        final_name,
        parent_dirs_to_sync,
    ))
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
/// Returns the operating-system error reported by `openat`.
///
/// # Panics
///
/// Panics if `name` violates the validated-component invariant by containing
/// an interior NUL.
#[inline]
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
/// * `parent` - The open parent-directory handle used by `mkdirat`.
/// * `name` - The single native path component to create.
/// * `diagnostic_path` - The path attached to a creation error.
///
/// # Returns
///
/// `Ok(())` after creating the directory or observing that it already exists.
///
/// # Errors
///
/// Returns a contextual operating-system error when `mkdirat` fails for a
/// reason other than an existing entry.
///
/// # Panics
///
/// Panics if `name` violates the validated-component invariant by containing
/// an interior NUL.
#[inline]
fn create_directory_at(
    parent: &File,
    name: &OsStr,
    diagnostic_path: &Path,
) -> Result<()> {
    let name = component_c_string(name);
    // SAFETY: the parent descriptor and NUL-terminated component remain live;
    // `mkdirat` does not retain either value.
    let result =
        unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    normalize_mkdirat_result(result, diagnostic_path)
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
        return missing_rooted_entry(Error::last_os_error(), diagnostic_path);
    }
    // SAFETY: successful `fstatat` initialized the complete `stat` value.
    let status = unsafe { status.assume_init() };
    if !super::unix_stat::is_regular_file_mode(status.st_mode) {
        return Err(rooted_type_error(diagnostic_path, "regular file"));
    }
    Ok(())
}

/// Verifies that an opened handle is a directory.
///
/// # Parameters
///
/// * `directory` - The open handle to inspect.
/// * `operation` - The operation label attached to metadata errors.
/// * `diagnostic_path` - The path attached to errors and type diagnostics.
///
/// # Returns
///
/// `Ok(())` when the open handle refers to a directory.
///
/// # Errors
///
/// Returns a contextual metadata error or `InvalidInput` for a non-directory.
#[inline]
fn verify_opened_directory(
    directory: &File,
    operation: &'static str,
    diagnostic_path: &Path,
) -> Result<()> {
    normalize_opened_directory_metadata(
        directory.metadata(),
        operation,
        diagnostic_path,
    )
}

/// Verifies an opened rooted file and restores blocking behavior.
///
/// # Parameters
///
/// * `file` - The opened rooted file handle to inspect and normalize.
/// * `restore_operation` - The operation label attached to descriptor errors.
/// * `diagnostic_path` - The path attached to metadata and descriptor errors.
///
/// # Returns
///
/// `Ok(())` after a regular file handle has been verified and normalized to
/// blocking mode.
///
/// # Errors
///
/// Returns a contextual metadata or descriptor error, or `InvalidInput` for a
/// non-regular handle.
#[inline]
fn prepare_opened_rooted_regular_file(
    file: &File,
    restore_operation: &'static str,
    diagnostic_path: &Path,
) -> Result<()> {
    normalize_opened_regular_file_metadata(file.metadata(), diagnostic_path)
        .and_then(|()| {
            with_path_context(
                clear_nonblocking(file.as_raw_fd()),
                restore_operation,
                diagnostic_path,
            )
        })
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
/// # Panics
///
/// Panics if the validated component unexpectedly contains an interior NUL.
#[must_use]
#[inline]
fn component_c_string(component: &OsStr) -> CString {
    CString::new(component.as_bytes())
        .expect("LocalRelativePath guarantees components without NUL")
}

/// Adds rooted-open normalization and path context to an open result.
///
/// # Type Parameters
///
/// * `T` - The successful value carried through unchanged.
///
/// # Parameters
///
/// * `result` - The rooted-open result to return or normalize.
/// * `operation` - The operation description attached to an error.
/// * `path` - The diagnostic path attached to an error.
///
/// # Returns
///
/// The original success value, or a normalized and contextualized error.
///
/// # Errors
///
/// Returns a contextual error when `result` is `Err`, normalizing stable link
/// and wrong-directory failures to `InvalidInput`.
#[inline]
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
#[inline]
pub(super) fn rooted_type_error(path: &Path, expected: &str) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("rooted path is not a {expected}: {}", path.display(),),
    )
}
