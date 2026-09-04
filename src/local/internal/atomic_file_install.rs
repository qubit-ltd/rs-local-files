// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic existing-file replacement and no-replace installation.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

#[cfg(unix)]
use std::ffi::CStr;
#[cfg(unix)]
use std::ffi::CString;
use std::io::Error;
#[cfg(unix)]
use std::io::ErrorKind;
use std::io::Result;
#[cfg(unix)]
use std::os::fd::RawFd;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

use super::atomic_staging_state::AtomicStagingState;
#[cfg(not(unix))]
use super::file_move::move_file_without_replacing;
#[cfg(not(windows))]
use super::file_move::replace_file;
#[cfg(windows)]
use super::file_move::wide_path;
use crate::LocalAtomicDestinationState;

/// Installs a staged atomic file according to its initial destination state.
pub(crate) fn install_atomic_file(
    staging: &Path,
    destination: &Path,
    destination_existed: bool,
) -> std::result::Result<(), (Error, LocalAtomicDestinationState, AtomicStagingState)> {
    if destination_existed {
        match replace_existing_atomic_file(staging, destination) {
            Ok(()) => Ok(()),
            Err(source) => {
                let destination_state = replacement_error_state(&source);
                let staging_state = if destination_state == LocalAtomicDestinationState::Unchanged {
                    AtomicStagingState::Present
                } else {
                    AtomicStagingState::Indeterminate
                };
                Err((source, destination_state, staging_state))
            }
        }
    } else {
        install_new_atomic_file(staging, destination)
    }
}

/// Atomically replaces an existing destination file.
pub(crate) fn replace_existing_atomic_file(staging: &Path, destination: &Path) -> Result<()> {
    #[cfg(all(feature = "test-support", unix))]
    if super::test_support::is_enabled("atomic-install-replace")
        || super::test_support::is_enabled("atomic-install-replace-indeterminate")
    {
        return Err(crate::local::test_fault_error());
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
) -> std::result::Result<(), (Error, LocalAtomicDestinationState, AtomicStagingState)> {
    #[cfg(unix)]
    {
        let staging = native_path(staging).map_err(unchanged_error)?;
        let destination = native_path(destination).map_err(unchanged_error)?;
        install_new_atomic_file_at(libc::AT_FDCWD, &staging, libc::AT_FDCWD, &destination)
    }
    #[cfg(not(unix))]
    {
        move_file_without_replacing(staging, destination).map_err(unchanged_error)
    }
}

/// Classifies a native existing-file replacement failure.
pub(crate) fn replacement_error_state(error: &Error) -> LocalAtomicDestinationState {
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("atomic-install-replace-indeterminate")
        || super::test_support::is_enabled("rooted-install-indeterminate")
    {
        return LocalAtomicDestinationState::Indeterminate;
    }
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
) -> std::result::Result<(), (Error, LocalAtomicDestinationState, AtomicStagingState)> {
    #[cfg(feature = "test-support")]
    if super::test_support::is_enabled("atomic-install-before-native-call") {
        return Err(unchanged_error(crate::local::test_fault_error()));
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        #[cfg(feature = "test-support")]
        let forced_fallback = [
            "atomic-install-fallback",
            "atomic-install-link",
            "atomic-install-unlink",
            "atomic-install-unlink-persistent",
            "atomic-install-unlink-persistent-sync",
            "atomic-install-unlink-recover-sync",
            "atomic-install-unlink-indeterminate-sync",
        ]
        .into_iter()
        .any(super::test_support::is_enabled);
        #[cfg(not(feature = "test-support"))]
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
        if code != Some(libc::ENOSYS) && code != Some(libc::EINVAL) && code != Some(libc::EOPNOTSUPP) {
            return Err(unchanged_error(error));
        }
        link_then_unlink(staging_parent, staging, destination_parent, destination)
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
        link_then_unlink(staging_parent, staging, destination_parent, destination)
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
///
/// # Parameters
///
/// * `staging_parent` - Directory descriptor authorizing the staging name.
/// * `staging` - Staging entry name relative to `staging_parent`.
/// * `destination_parent` - Directory descriptor authorizing the destination.
/// * `destination` - Destination entry name relative to `destination_parent`.
///
/// # Returns
///
/// `Ok(())` after the hard link is published and the staging name is removed.
///
/// # Errors
///
/// Returns the native link or unlink error together with the known destination
/// and staging states.
///
/// # Panics
///
/// Panics if the internal unlink-attempt count is configured as zero.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd",))]
fn link_then_unlink(
    staging_parent: RawFd,
    staging: &CStr,
    destination_parent: RawFd,
    destination: &CStr,
) -> std::result::Result<(), (Error, LocalAtomicDestinationState, AtomicStagingState)> {
    #[cfg(feature = "test-support")]
    let forced_link_error = super::test_support::is_enabled("atomic-install-link");
    #[cfg(not(feature = "test-support"))]
    let forced_link_error = false;
    // SAFETY: both directory descriptors and names remain live for this
    // non-retaining hard-link operation.
    let link_result = if forced_link_error {
        -1
    } else {
        unsafe {
            libc::linkat(
                staging_parent,
                staging.as_ptr(),
                destination_parent,
                destination.as_ptr(),
                0,
            )
        }
    };
    if link_result == -1 {
        let error = if forced_link_error {
            crate::local::test_fault_error()
        } else {
            Error::last_os_error()
        };
        return Err(unchanged_error(error));
    }
    const MAX_UNLINK_ATTEMPTS: usize = 2;
    let mut last_error = None;
    for _ in 0..MAX_UNLINK_ATTEMPTS {
        match unlink_staging_name(staging_parent, staging) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    #[cfg(feature = "test-support")]
    let staging_state = if super::test_support::is_enabled("atomic-install-unlink-indeterminate-sync") {
        AtomicStagingState::Indeterminate
    } else {
        AtomicStagingState::Present
    };
    #[cfg(not(feature = "test-support"))]
    let staging_state = AtomicStagingState::Present;
    Err((
        last_error.expect("at least one staging unlink attempt should run"),
        LocalAtomicDestinationState::Replaced,
        staging_state,
    ))
}

/// Removes the staging name after its destination hard link is published.
///
/// # Parameters
///
/// * `staging_parent` - Directory descriptor authorizing the staging name.
/// * `staging` - Staging entry name relative to `staging_parent`.
///
/// # Errors
///
/// Returns the native `unlinkat` error or a selected test fault.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd",))]
fn unlink_staging_name(staging_parent: RawFd, staging: &CStr) -> Result<()> {
    #[cfg(feature = "test-support")]
    let forced_unlink_error = super::test_support::take("atomic-install-unlink")
        || super::test_support::is_enabled("atomic-install-unlink-persistent")
        || super::test_support::is_enabled("atomic-install-unlink-persistent-sync")
        || super::test_support::is_enabled("atomic-install-unlink-recover-sync")
        || super::test_support::is_enabled("atomic-install-unlink-indeterminate-sync");
    #[cfg(not(feature = "test-support"))]
    let forced_unlink_error = false;
    // SAFETY: the staging directory descriptor and name remain live for this
    // non-retaining unlink operation.
    let result = if forced_unlink_error {
        -1
    } else {
        unsafe { libc::unlinkat(staging_parent, staging.as_ptr(), 0) }
    };
    if result == -1 {
        return Err(if forced_unlink_error {
            crate::local::test_fault_error()
        } else {
            Error::last_os_error()
        });
    }
    Ok(())
}

/// Converts a Unix path to a NUL-terminated byte string.
#[cfg(unix)]
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn native_path(path: &Path) -> Result<CString> {
    match CString::new(path.as_os_str().as_bytes()) {
        Ok(path) => Ok(path),
        Err(_) => Err(Error::new(ErrorKind::InvalidInput, "atomic install path contains NUL")),
    }
}

/// Pairs an error with a destination known to be unmodified.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn unchanged_error(error: Error) -> (Error, LocalAtomicDestinationState, AtomicStagingState) {
    (
        error,
        LocalAtomicDestinationState::Unchanged,
        AtomicStagingState::Present,
    )
}
