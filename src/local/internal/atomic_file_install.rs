// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic existing-file replacement and no-replace installation.
// qubit-style: allow source-test-pair
// qubit-style: allow coverage-cfg
// Private behavior is covered through public integration tests.

#[cfg(unix)]
use std::ffi::{
    CStr,
    CString,
};
#[cfg(unix)]
use std::io::ErrorKind;
use std::io::{
    Error,
    Result,
};
#[cfg(unix)]
use std::os::fd::RawFd;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

use crate::LocalAtomicDestinationState;

#[cfg(not(unix))]
use super::file_move::move_file_without_replacing;
#[cfg(not(windows))]
use super::file_move::replace_file;
#[cfg(windows)]
use super::file_move::wide_path;

/// Installs a staged atomic file according to its initial destination state.
pub(crate) fn install_atomic_file(
    staging: &Path,
    destination: &Path,
    destination_existed: bool,
) -> std::result::Result<(), (Error, LocalAtomicDestinationState)> {
    if destination_existed {
        match replace_existing_atomic_file(staging, destination) {
            Ok(()) => Ok(()),
            Err(source) => {
                let destination_state = replacement_error_state(&source);
                Err((source, destination_state))
            }
        }
    } else {
        install_new_atomic_file(staging, destination)
    }
}

/// Atomically replaces an existing destination file.
pub(crate) fn replace_existing_atomic_file(
    staging: &Path,
    destination: &Path,
) -> Result<()> {
    #[cfg(all(coverage, unix))]
    if super::coverage_fault::is_enabled("atomic-install-replace") {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    #[cfg(not(windows))]
    {
        replace_file(staging, destination)
    }
    #[cfg(windows)]
    {
        let staging = wide_path(staging)?;
        let destination = wide_path(destination)?;
        // SAFETY: both path buffers are NUL-terminated, contain no interior
        // NUL, and remain live for this non-retaining call. Null backup and
        // exclusion pointers and zero flags request strict native merging.
        let result = unsafe {
            ReplaceFileW(
                destination.as_ptr(),
                staging.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if result == 0 {
            return Err(Error::last_os_error());
        }
        Ok(())
    }
}

/// Installs a new destination without replacing a concurrent entry.
pub(crate) fn install_new_atomic_file(
    staging: &Path,
    destination: &Path,
) -> std::result::Result<(), (Error, LocalAtomicDestinationState)> {
    #[cfg(unix)]
    {
        let staging = native_path(staging).map_err(unchanged_error)?;
        let destination = native_path(destination).map_err(unchanged_error)?;
        install_new_atomic_file_at(
            libc::AT_FDCWD,
            &staging,
            libc::AT_FDCWD,
            &destination,
        )
    }
    #[cfg(not(unix))]
    {
        move_file_without_replacing(staging, destination)
            .map_err(unchanged_error)
    }
}

/// Classifies a native existing-file replacement failure.
pub(crate) fn replacement_error_state(
    error: &Error,
) -> LocalAtomicDestinationState {
    #[cfg(windows)]
    {
        match error.raw_os_error() {
            Some(1175) => LocalAtomicDestinationState::Unchanged,
            Some(1176) => LocalAtomicDestinationState::Missing,
            Some(1177) => LocalAtomicDestinationState::Indeterminate,
            _ => LocalAtomicDestinationState::Unchanged,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        LocalAtomicDestinationState::Unchanged
    }
}

/// Installs a new descriptor-relative destination without replacement.
#[cfg(unix)]
pub(crate) fn install_new_atomic_file_at(
    staging_parent: RawFd,
    staging: &CStr,
    destination_parent: RawFd,
    destination: &CStr,
) -> std::result::Result<(), (Error, LocalAtomicDestinationState)> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        #[cfg(coverage)]
        if super::coverage_fault::is_enabled(
            "atomic-install-before-native-call",
        ) {
            return Err(unchanged_error(Error::from_raw_os_error(libc::EIO)));
        }
        #[cfg(coverage)]
        let forced_fallback = [
            "atomic-install-fallback",
            "atomic-install-link",
            "atomic-install-unlink",
        ]
        .into_iter()
        .any(super::coverage_fault::is_enabled);
        #[cfg(not(coverage))]
        let forced_fallback = false;
        let result = if forced_fallback {
            -1
        } else {
            // SAFETY: both directory descriptors and C strings remain live
            // for the non-retaining syscall, and `RENAME_NOREPLACE` is the
            // only flag.
            unsafe {
                libc::syscall(
                    libc::SYS_renameat2,
                    staging_parent,
                    staging.as_ptr(),
                    destination_parent,
                    destination.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            }
        };
        if result == 0 {
            return Ok(());
        }
        let error = if forced_fallback {
            Error::from_raw_os_error(libc::ENOSYS)
        } else {
            Error::last_os_error()
        };
        let code = error.raw_os_error();
        if code != Some(libc::ENOSYS)
            && code != Some(libc::EINVAL)
            && code != Some(libc::EOPNOTSUPP)
        {
            return Err(unchanged_error(error));
        }
        link_then_unlink(
            staging_parent,
            staging,
            destination_parent,
            destination,
        )
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: both directory descriptors and C strings remain live for
        // this non-retaining exclusive rename operation.
        let result = unsafe {
            libc::renameatx_np(
                staging_parent,
                staging.as_ptr(),
                destination_parent,
                destination.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if result == -1 {
            return Err(unchanged_error(Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(target_os = "freebsd")]
    {
        link_then_unlink(
            staging_parent,
            staging,
            destination_parent,
            destination,
        )
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "freebsd",
    )))]
    {
        let _ = (staging_parent, staging, destination_parent, destination);
        Err(unchanged_error(Error::new(
            ErrorKind::Unsupported,
            "atomic no-replace installation is unsupported on this target",
        )))
    }
}

/// Creates a new hard link and then removes the staging name.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd",))]
fn link_then_unlink(
    staging_parent: RawFd,
    staging: &CStr,
    destination_parent: RawFd,
    destination: &CStr,
) -> std::result::Result<(), (Error, LocalAtomicDestinationState)> {
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("atomic-install-link") {
        return Err(unchanged_error(Error::from_raw_os_error(libc::EIO)));
    }
    // SAFETY: both directory descriptors and names remain live for this
    // non-retaining hard-link operation.
    let link_result = unsafe {
        libc::linkat(
            staging_parent,
            staging.as_ptr(),
            destination_parent,
            destination.as_ptr(),
            0,
        )
    };
    if link_result == -1 {
        return Err(unchanged_error(Error::last_os_error()));
    }
    #[cfg(coverage)]
    let forced_unlink_error =
        super::coverage_fault::is_enabled("atomic-install-unlink");
    #[cfg(not(coverage))]
    let forced_unlink_error = false;
    let unlink_result = if forced_unlink_error {
        -1
    } else {
        // SAFETY: the staging directory descriptor and name remain live for
        // this non-retaining unlink operation.
        unsafe { libc::unlinkat(staging_parent, staging.as_ptr(), 0) }
    };
    if unlink_result == -1 {
        return Err((
            if forced_unlink_error {
                Error::from_raw_os_error(libc::EIO)
            } else {
                Error::last_os_error()
            },
            LocalAtomicDestinationState::Replaced,
        ));
    }
    Ok(())
}

/// Converts a Unix path to a NUL-terminated byte string.
#[cfg(unix)]
fn native_path(path: &Path) -> Result<CString> {
    match CString::new(path.as_os_str().as_bytes()) {
        Ok(path) => Ok(path),
        Err(_) => Err(Error::new(
            ErrorKind::InvalidInput,
            "atomic install path contains NUL",
        )),
    }
}

/// Pairs an error with a destination known to be unmodified.
fn unchanged_error(error: Error) -> (Error, LocalAtomicDestinationState) {
    (error, LocalAtomicDestinationState::Unchanged)
}
