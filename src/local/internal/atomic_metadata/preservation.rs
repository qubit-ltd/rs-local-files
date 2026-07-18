// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Common strict metadata-preservation orchestration.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs::File;
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;

#[cfg(target_os = "freebsd")]
use super::freebsd::preserve_extended_metadata;
#[cfg(any(target_os = "linux", target_os = "android"))]
use super::linux_android::preserve_extended_metadata;
#[cfg(target_os = "macos")]
use super::macos::preserve_extended_metadata;

/// Copies owner, mode, ACLs, labels, and extended attributes to staging.
///
/// Owner is applied before mode because `fchown` may clear special mode bits.
/// Extended metadata is applied last so ownership changes cannot clear copied
/// security attributes.
///
/// # Parameters
///
/// * `source` - Open destination snapshot whose metadata must be retained.
/// * `staging` - Open staging file receiving the metadata.
///
/// # Errors
///
/// Returns the first native error from metadata inspection or application.
pub(crate) fn preserve_atomic_metadata(
    source: &File,
    staging: &File,
) -> Result<()> {
    #[cfg(coverage)]
    let source_metadata = if super::super::coverage_fault::is_enabled(
        "atomic-metadata-source-stat",
    ) {
        Err(Error::from_raw_os_error(libc::EIO))
    } else {
        source.metadata()
    }?;
    #[cfg(not(coverage))]
    let source_metadata = source.metadata()?;
    #[cfg(coverage)]
    let staging_metadata = if super::super::coverage_fault::is_enabled(
        "atomic-metadata-staging-stat",
    ) {
        Err(Error::from_raw_os_error(libc::EIO))
    } else {
        staging.metadata()
    }?;
    #[cfg(not(coverage))]
    let staging_metadata = staging.metadata()?;
    #[cfg(coverage)]
    let forced_owner_error =
        super::super::coverage_fault::is_enabled("atomic-metadata-owner");
    #[cfg(coverage)]
    let forced_owner_native_error = super::super::coverage_fault::is_enabled(
        "atomic-metadata-owner-native",
    );
    #[cfg(not(coverage))]
    let forced_owner_error = false;
    #[cfg(not(coverage))]
    let forced_owner_native_error = false;
    if forced_owner_error
        || forced_owner_native_error
        || source_metadata.uid() != staging_metadata.uid()
        || source_metadata.gid() != staging_metadata.gid()
    {
        // SAFETY: the staging descriptor remains live and the uid/gid values
        // came from metadata for a live Unix file handle.
        let result = if forced_owner_native_error {
            -1
        } else {
            unsafe {
                libc::fchown(
                    staging.as_raw_fd(),
                    source_metadata.uid(),
                    source_metadata.gid(),
                )
            }
        };
        if forced_owner_error || result == -1 {
            return Err(if forced_owner_error {
                Error::from_raw_os_error(libc::EIO)
            } else {
                Error::last_os_error()
            });
        }
    }
    let mode = native_mode(source_metadata.mode())?;
    #[cfg(coverage)]
    let forced_mode_error =
        super::super::coverage_fault::is_enabled("atomic-metadata-mode");
    #[cfg(not(coverage))]
    let forced_mode_error = false;
    // SAFETY: the staging descriptor remains live and `mode` contains the
    // native Unix mode bits expected by `fchmod`.
    let result = unsafe { libc::fchmod(staging.as_raw_fd(), mode) };
    if forced_mode_error || result == -1 {
        return Err(if forced_mode_error {
            Error::from_raw_os_error(libc::EIO)
        } else {
            Error::last_os_error()
        });
    }
    preserve_extended_metadata(source, staging)
}

/// Converts portable metadata mode bits to the platform-native mode type.
fn native_mode<T>(mode: u32) -> Result<T>
where
    T: TryFrom<u32>,
{
    #[cfg(coverage)]
    let mode = if super::super::coverage_fault::is_enabled(
        "atomic-metadata-native-mode",
    ) {
        None
    } else {
        T::try_from(mode).ok()
    };
    #[cfg(not(coverage))]
    let mode = T::try_from(mode).ok();
    match mode {
        Some(mode) => Ok(mode),
        None => Err(Error::new(
            ErrorKind::InvalidData,
            "source mode cannot be represented by the target platform",
        )),
    }
}

/// Rejects strict metadata replacement on an unsupported Unix platform.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
)))]
fn preserve_extended_metadata(_source: &File, _staging: &File) -> Result<()> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "strict atomic metadata preservation is unsupported on this target",
    ))
}
