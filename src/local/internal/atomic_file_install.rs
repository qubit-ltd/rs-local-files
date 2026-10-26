// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic existing-file replacement and no-replace installation.
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
use crate::options::LocalWriteMetadataPolicy;

/// Installs a staged atomic file using the operation selected from the
/// destination state observed before staging began.
///
/// Existing destinations are replaced atomically; on Windows, staging
/// metadata is also selected according to `metadata_policy`. New destinations
/// are installed without replacing an entry created concurrently. The result
/// includes destination and staging states so callers can recover safely.
///
/// # Parameters
///
/// * `staging` - Path of the fully written temporary file to publish.
/// * `destination` - Path that receives the staged file.
/// * `destination_existed` - Whether the destination existed before staging.
/// * `metadata_policy` - Metadata behavior for Windows replacement.
///
/// # Errors
///
/// Returns the operating-system error together with conservative destination
/// and staging states describing what is known after the failed operation.
pub(crate) fn install_atomic_file(
    staging: &Path,
    destination: &Path,
    destination_existed: bool,
    metadata_policy: LocalWriteMetadataPolicy,
) -> std::result::Result<(), (Error, LocalAtomicDestinationState, AtomicStagingState)> {
    if destination_existed {
        #[cfg(windows)]
        if metadata_policy == LocalWriteMetadataPolicy::UseStaging {
            return super::file_move::replace_file(staging, destination).map_err(unchanged_error);
        }
        #[cfg(not(windows))]
        let _ = metadata_policy;
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

/// Atomically replaces an existing destination file with the staged file.
///
/// On Windows this uses the native replacement operation; on other platforms
/// it delegates to the platform-specific file replacement helper. The staged
/// path must identify a file on the same filesystem as the destination.
///
/// # Parameters
///
/// * `staging` - Path of the staged file.
/// * `destination` - Existing path to replace.
///
/// # Returns
///
/// Returns `Ok(())` after replacement succeeds.
///
/// # Errors
///
/// Returns the underlying replacement error when the operating system cannot
/// complete the replacement.
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

/// Installs a new destination without replacing an entry created concurrently.
///
/// Unix targets use a no-replace native operation relative to the current
/// working directory; other targets use the platform file-move helper. The
/// caller receives conservative destination and staging states on failure.
///
/// # Parameters
///
/// * `staging` - Path of the fully written temporary file to publish.
/// * `destination` - Path that must not be replaced if it already exists.
///
/// # Errors
///
/// Returns the native error and the destination/staging states known after
/// installation fails.
///
/// # Returns
///
/// Returns `Ok(())` after installation or an error with the known filesystem
/// states when installation fails.
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

/// Classifies the destination state reported by a native replacement failure.
///
/// Windows replacement error codes distinguish an unchanged destination, a
/// missing destination, and an indeterminate outcome. Other platforms report
/// the destination as unchanged because their replacement helper does not
/// expose a more precise state.
///
/// # Parameters
///
/// * `error` - Native error returned by the replacement operation.
///
/// # Returns
///
/// Returns the most precise destination state available on this platform.
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

/// Installs a new descriptor-relative destination without replacing an entry.
///
/// Linux and Android first attempt `renameat2` with `RENAME_NOREPLACE`, then
/// fall back to linking and unlinking when that operation is unavailable.
/// macOS uses `renameatx_np` with `RENAME_EXCL`; FreeBSD uses the link/unlink
/// fallback. Other Unix targets return `Unsupported`.
///
/// # Errors
///
/// Returns the native operation error paired with the destination and staging
/// states known after the failure.
///
/// # Parameters
///
/// * `staging_parent` - Open directory containing the staging name.
/// * `staging` - NUL-terminated staging entry name.
/// * `destination_parent` - Open directory containing the destination name.
/// * `destination` - NUL-terminated destination entry name.
///
/// # Returns
///
/// Returns `Ok(())` after installation, or the failure and known states.
///
/// # Panics
///
/// The link/unlink fallback panics only if its compile-time unlink attempt
/// count is changed to zero.
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
            "atomic-install-unlink-recover-success",
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

/// Publishes the destination as a hard link, then removes the staging name.
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

/// Removes the staging name after its destination hard link has been published.
///
/// # Parameters
///
/// * `staging_parent` - Directory descriptor authorizing the staging name.
/// * `staging` - Staging entry name relative to `staging_parent`.
///
/// # Errors
///
/// Returns the native `unlinkat` error or a selected test fault.
///
/// # Returns
///
/// Returns `Ok(())` after the staging name is removed.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd",))]
fn unlink_staging_name(staging_parent: RawFd, staging: &CStr) -> Result<()> {
    #[cfg(feature = "test-support")]
    let forced_unlink_error = super::test_support::take("atomic-install-unlink")
        || super::test_support::is_enabled("atomic-install-unlink-persistent")
        || super::test_support::is_enabled("atomic-install-unlink-persistent-sync")
        || super::test_support::is_enabled("atomic-install-unlink-recover-sync")
        || super::test_support::is_enabled("atomic-install-unlink-recover-success")
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

/// Converts a Unix path's raw bytes to a NUL-terminated byte string.
///
/// This preserves non-UTF-8 path bytes. Paths containing an interior NUL are
/// rejected because native path-taking system calls use NUL termination.
///
/// # Errors
///
/// Returns `InvalidInput` when the path contains an interior NUL byte.
///
/// # Returns
///
/// Returns the raw path bytes as a `CString` when they contain no interior NUL.
#[cfg(unix)]
#[inline]
fn native_path(path: &Path) -> Result<CString> {
    match CString::new(path.as_os_str().as_bytes()) {
        Ok(path) => Ok(path),
        Err(_) => Err(Error::new(ErrorKind::InvalidInput, "atomic install path contains NUL")),
    }
}

/// Pairs an error with states indicating that the destination is unchanged
/// and the staging file is still present.
///
/// # Parameters
///
/// * `error` - The failure that occurred before destination publication.
///
/// # Returns
///
/// Returns the error paired with `Unchanged` and `Present` states.
#[inline]
fn unchanged_error(error: Error) -> (Error, LocalAtomicDestinationState, AtomicStagingState) {
    (
        error,
        LocalAtomicDestinationState::Unchanged,
        AtomicStagingState::Present,
    )
}
