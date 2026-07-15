// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private platform-specific file moves and parent synchronization.

#[cfg(unix)]
use std::ffi::CString;
#[cfg(windows)]
use std::ffi::c_void;
#[cfg(not(windows))]
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::io::{FromRawHandle, RawHandle};

#[cfg(windows)]
const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
#[cfg(windows)]
const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
#[cfg(windows)]
const GENERIC_READ: u32 = 0x8000_0000;
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;
#[cfg(windows)]
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
#[cfg(windows)]
const FILE_SHARE_DELETE: u32 = 0x0000_0004;
#[cfg(windows)]
const OPEN_EXISTING: u32 = 3;
#[cfg(windows)]
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;
#[cfg(target_os = "macos")]
const RENAME_EXCL: std::os::raw::c_uint = 0x0000_0004;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn renamex_np(
        from: *const std::os::raw::c_char,
        to: *const std::os::raw::c_char,
        flags: std::os::raw::c_uint,
    ) -> std::os::raw::c_int;
}

#[cfg(windows)]
unsafe extern "system" {
    fn MoveFileExW(existing_file_name: *const u16, new_file_name: *const u16, flags: u32) -> i32;

    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: RawHandle,
    ) -> RawHandle;
}

/// Replaces `destination` with `source`.
///
/// # Parameters
/// - `source`: Existing temporary file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Returns the platform I/O error reported while replacing the destination.
#[cfg(not(windows))]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)
}

/// Replaces `destination` with `source`.
///
/// # Parameters
/// - `source`: Existing temporary file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Returns the platform I/O error reported while replacing the destination.
#[cfg(windows)]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    let source = wide_path(source)?;
    let destination = wide_path(destination)?;
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Moves `source` to `destination` without replacing an existing destination.
///
/// # Parameters
/// - `source`: Existing source path.
/// - `destination`: Destination path.
///
/// # Errors
/// Returns the platform I/O error reported while moving the path.
#[cfg(target_os = "macos")]
pub(crate) fn move_path_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    let source = c_path(source)?;
    let destination = c_path(destination)?;
    let result = unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Moves `source` to `destination` without replacing an existing destination.
///
/// # Parameters
/// - `source`: Existing source path.
/// - `destination`: Destination path.
///
/// # Errors
/// Returns the platform I/O error reported while moving the path.
#[cfg(target_os = "linux")]
pub(crate) fn move_path_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    let source = c_path(source)?;
    let destination = c_path(destination)?;
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Moves `source` to `destination` without replacing an existing destination.
///
/// # Parameters
/// - `source`: Existing source path.
/// - `destination`: Destination path.
///
/// # Errors
/// Returns the platform I/O error reported while moving the path.
#[cfg(windows)]
pub(crate) fn move_path_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    let source = wide_path(source)?;
    let destination = wide_path(destination)?;
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Moves `source` to `destination` without replacing an existing file.
///
/// # Parameters
/// - `source`: Existing source file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Returns the platform I/O error reported while moving the file.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn move_file_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    move_path_without_replacing(source, destination)
}

/// Moves a directory without replacing an existing destination.
///
/// # Parameters
/// - `source`: Existing source directory path.
/// - `destination`: Destination directory path.
///
/// # Errors
/// Returns the platform I/O error reported while moving the directory.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn move_directory_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    move_path_without_replacing(source, destination)
}

/// Rejects no-replace directory persistence on unsupported targets.
///
/// # Parameters
/// - `source`: Existing source directory path.
/// - `destination`: Destination directory path.
///
/// # Errors
/// Always returns [`ErrorKind::Unsupported`] because this target has no native
/// no-replace directory move implementation.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn move_directory_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    Err(Error::new(
        ErrorKind::Unsupported,
        format!(
            "moving directory '{}' to '{}' without replacement is unsupported",
            source.display(),
            destination.display()
        ),
    ))
}

/// Moves `source` to `destination` without replacing an existing file.
///
/// This fallback creates a hard link at the destination, then removes the
/// original temporary file. The destination creation is atomic and fails when
/// the destination already exists.
///
/// # Parameters
/// - `source`: Existing source file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Returns the platform I/O error reported while linking, unlinking, or
/// rolling back the destination link after an unlink failure.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub(crate) fn move_file_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    fs::hard_link(source, destination)?;
    match fs::remove_file(source) {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Err(cleanup_error) = fs::remove_file(destination) {
                return Err(Error::new(
                    error.kind(),
                    format!(
                        "failed to remove source after linking destination: {error}; \
                         additionally failed to remove destination '{}': {cleanup_error}",
                        destination.display(),
                    ),
                ));
            }
            Err(error)
        }
    }
}

/// Converts a Unix path to a C string.
///
/// # Parameters
/// - `path`: Path to convert.
///
/// # Returns
/// A nul-terminated C path string.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when the path contains an interior NUL.
#[cfg(unix)]
fn c_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("path contains an interior NUL byte: {}", path.display()),
        )
    })
}

/// Syncs the parent directory for `path`.
///
/// # Parameters
/// - `path`: File path whose parent directory should be synced.
///
/// # Errors
/// Returns an I/O error when opening or syncing the parent directory fails.
#[cfg(not(windows))]
pub(super) fn sync_parent_dir(path: &Path) -> Result<()> {
    let parent_dir = parent_dir_for(path);
    let parent = File::open(parent_dir)?;
    parent.sync_all()
}

/// Syncs the parent directory for `path`.
///
/// # Parameters
/// - `path`: File path whose parent directory should be synced.
///
/// # Errors
/// Returns an I/O error when opening or syncing the parent directory fails.
#[cfg(windows)]
pub(super) fn sync_parent_dir(path: &Path) -> Result<()> {
    let parent = wide_path(parent_dir_for(path))?;
    let handle = unsafe {
        CreateFileW(
            parent.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = std::io::Error::last_os_error();
        return if is_ignorable_windows_parent_sync_error(&error) {
            Ok(())
        } else {
            Err(error)
        };
    }
    let directory = unsafe { File::from_raw_handle(handle) };
    match directory.sync_all() {
        Ok(()) => Ok(()),
        Err(error) if is_ignorable_windows_parent_sync_error(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Tests whether a Windows parent-directory sync error should be ignored.
///
/// # Parameters
/// - `error`: Error reported while opening or syncing the parent directory.
///
/// # Returns
/// `true` when the error only means the best-effort parent directory sync is
/// unavailable on Windows.
#[cfg(windows)]
fn is_ignorable_windows_parent_sync_error(error: &Error) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;

    error.kind() == ErrorKind::PermissionDenied
        || error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
}

/// Gets the parent directory that should be synced for `path`.
///
/// # Parameters
/// - `path`: File path whose parent directory is needed.
///
/// # Returns
/// The parent directory, or the current directory for parentless paths.
pub(super) fn parent_dir_for(path: &Path) -> &Path {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        return parent;
    }
    Path::new(".")
}

/// Converts a path into a null-terminated Windows wide string.
///
/// # Parameters
/// - `path`: Path to convert.
///
/// # Returns
/// Null-terminated UTF-16 path buffer.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `path` contains an interior NUL.
#[cfg(windows)]
fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path contains an interior NUL: {}", path.display()),
        ));
    }
    Ok(units.into_iter().chain(Some(0)).collect())
}
