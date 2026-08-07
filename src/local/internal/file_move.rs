// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private platform-specific file moves and parent synchronization.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.
//!
//! Windows paths are passed to native APIs as their existing UTF-16 spelling
//! plus a terminating NUL. This module rejects interior NULs, but does not add
//! a verbatim-path prefix, convert relative paths to absolute paths, or
//! otherwise change platform path-length and path-resolution semantics.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::CString;
#[cfg(not(windows))]
use std::fs;
use std::fs::File;
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::io::{
    AsRawHandle,
    FromRawHandle,
};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    GENERIC_READ,
    INVALID_HANDLE_VALUE,
};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW,
    DELETE,
    FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO,
    FILE_DISPOSITION_INFO,
    FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE,
    FILE_SHARE_READ,
    FILE_SHARE_WRITE,
    FileAttributeTagInfo,
    FileDispositionInfo,
    GetFileInformationByHandleEx,
    MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH,
    MoveFileExW,
    OPEN_EXISTING,
    SetFileInformationByHandle,
};

/// Windows reparse-tag bit identifying name-surrogate tags.
#[cfg(windows)]
const IO_REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;
/// macOS flag that prevents replacement during `renamex_np`.
#[cfg(target_os = "macos")]
const RENAME_EXCL: std::os::raw::c_uint = 0x0000_0004;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// Renames a path using macOS-specific flags.
    fn renamex_np(
        from: *const std::os::raw::c_char,
        to: *const std::os::raw::c_char,
        flags: std::os::raw::c_uint,
    ) -> std::os::raw::c_int;
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
#[inline(always)]
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
    // SAFETY: both UTF-16 buffers are NUL-terminated, contain no interior NUL,
    // and remain alive for the call. The flags are documented MoveFileExW
    // values and no aliasing or retained-pointer contract is involved.
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
pub(crate) fn move_path_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    #[cfg(feature = "internal-test-support")]
    if crate::local::test_support_enabled("persist-install-indeterminate") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "test-support-injected persistence install failure",
        ));
    }
    let source = c_path(source)?;
    let destination = c_path(destination)?;
    // SAFETY: both CString buffers are NUL-terminated and remain alive for the
    // call. `RENAME_EXCL` is a valid renamex_np flag and the function does not
    // retain either pointer.
    let result = unsafe {
        renamex_np(source.as_ptr(), destination.as_ptr(), RENAME_EXCL)
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
#[cfg(target_os = "linux")]
pub(crate) fn move_path_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    #[cfg(feature = "internal-test-support")]
    if crate::local::test_support_enabled("persist-install-indeterminate") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "test-support-injected persistence install failure",
        ));
    }
    let source = c_path(source)?;
    let destination = c_path(destination)?;
    // SAFETY: both CString pointers are valid and live for the syscall;
    // AT_FDCWD makes each path relative to the process current directory, and
    // RENAME_NOREPLACE is a valid renameat2 flag on Linux.
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
pub(crate) fn move_path_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    #[cfg(feature = "internal-test-support")]
    if crate::local::test_support_enabled("persist-install-indeterminate") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "test-support-injected persistence install failure",
        ));
    }
    let source = wide_path(source)?;
    let destination = wide_path(destination)?;
    // SAFETY: both UTF-16 buffers are NUL-terminated, contain no interior NUL,
    // and remain alive for the call. MOVEFILE_WRITE_THROUGH is a documented
    // MoveFileExW flag and the function does not retain either pointer.
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
#[inline(always)]
pub(crate) fn move_file_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    move_path_without_replacing(source, destination)
}

/// Rejects no-replace file persistence on unsupported targets.
///
/// # Parameters
/// - `source`: Existing source file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Always returns [`ErrorKind::Unsupported`] because this target has no native
/// no-replace file move implementation.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[inline]
pub(crate) fn move_file_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    Err(Error::new(
        ErrorKind::Unsupported,
        format!(
            "native no-replace installation from '{}' to '{}' is unsupported",
            source.display(),
            destination.display(),
        ),
    ))
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
#[inline(always)]
pub(crate) fn move_directory_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    move_path_without_replacing(source, destination)
}

/// Removes a Windows directory reparse point through a verified handle.
///
/// Opening the final component with `FILE_FLAG_OPEN_REPARSE_POINT` prevents
/// path traversal. The handle is then verified as both a directory and a
/// reparse point before it is marked for deletion, so a real directory that
/// replaces an earlier-observed link cannot be deleted by a path-level race.
///
/// # Parameters
/// - `path`: Directory symbolic link or directory reparse point to remove.
///
/// # Errors
/// Returns an I/O error when the path cannot be opened, inspected, or deleted.
/// Returns [`ErrorKind::AlreadyExists`] when the opened object is no longer a
/// directory reparse point.
#[cfg(windows)]
pub(crate) fn remove_directory_symlink(path: &Path) -> Result<()> {
    let path = wide_path(path)?;
    // SAFETY: `path` is a live NUL-terminated UTF-16 buffer without interior
    // NULs. The constants are documented CreateFileW access, share,
    // disposition, and reparse-point flags; null optional pointers are valid.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            DELETE | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(Error::last_os_error());
    }
    // SAFETY: the handle was checked against INVALID_HANDLE_VALUE and is a
    // uniquely owned CreateFileW result. File closes it exactly once.
    let entry = unsafe { File::from_raw_handle(handle) };
    let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `entry` owns a valid handle, `attributes` is a live writable
    // buffer of the advertised size, and FileAttributeTagInfo is the matching
    // structure for FILE_ATTRIBUTE_TAG_INFO_CLASS.
    let inspected = unsafe {
        GetFileInformationByHandleEx(
            entry.as_raw_handle(),
            FileAttributeTagInfo,
            std::ptr::from_mut(&mut attributes).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if inspected == 0 {
        return Err(Error::last_os_error());
    }
    let is_directory =
        attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    let is_reparse_point =
        attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    let is_name_surrogate =
        attributes.ReparseTag & IO_REPARSE_TAG_NAME_SURROGATE != 0;
    if !is_directory || !is_reparse_point || !is_name_surrogate {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "path no longer refers to a directory name-surrogate reparse point",
        ));
    }

    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `entry` owns a valid handle opened with DELETE access;
    // `disposition` is a live readable buffer of the advertised size and is
    // the matching structure for FILE_DISPOSITION_INFO_CLASS.
    let removed = unsafe {
        SetFileInformationByHandle(
            entry.as_raw_handle(),
            FileDispositionInfo,
            std::ptr::from_ref(&disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if removed == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
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
#[inline]
pub(crate) fn move_directory_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    Err(Error::new(
        ErrorKind::Unsupported,
        format!(
            "moving directory '{}' to '{}' without replacement is unsupported",
            source.display(),
            destination.display()
        ),
    ))
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
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[inline]
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
#[inline]
pub(crate) fn sync_parent_dir(path: &Path) -> Result<()> {
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
pub(crate) fn sync_parent_dir(path: &Path) -> Result<()> {
    let parent = wide_path(parent_dir_for(path))?;
    // SAFETY: `parent` is a live NUL-terminated UTF-16 buffer without interior
    // NULs; the remaining constants are documented CreateFileW access, share,
    // disposition, and directory flags. Null optional pointers are permitted.
    let handle = unsafe {
        CreateFileW(
            parent.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
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
    // SAFETY: the handle was checked against INVALID_HANDLE_VALUE and is a
    // uniquely owned CreateFileW result. Transferring it to File ensures it is
    // closed exactly once when `directory` is dropped.
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
#[must_use]
#[inline(always)]
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
#[must_use]
#[inline(always)]
pub(crate) fn parent_dir_for(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
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
#[inline]
pub(super) fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path contains an interior NUL: {}", path.display()),
        ));
    }
    Ok(units.into_iter().chain(Some(0)).collect())
}
