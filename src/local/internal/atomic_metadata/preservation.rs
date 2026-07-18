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
    let source_metadata = source.metadata()?;
    let staging_metadata = staging.metadata()?;
    if source_metadata.uid() != staging_metadata.uid()
        || source_metadata.gid() != staging_metadata.gid()
    {
        // SAFETY: the staging descriptor remains live and the uid/gid values
        // came from metadata for a live Unix file handle.
        let result = unsafe {
            libc::fchown(
                staging.as_raw_fd(),
                source_metadata.uid(),
                source_metadata.gid(),
            )
        };
        if result == -1 {
            return Err(Error::last_os_error());
        }
    }
    let mode = native_mode(source_metadata.mode())?;
    // SAFETY: the staging descriptor remains live and `mode` contains the
    // native Unix mode bits expected by `fchmod`.
    let result = unsafe { libc::fchmod(staging.as_raw_fd(), mode) };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    preserve_extended_metadata(source, staging)
}

/// Converts portable metadata mode bits to the platform-native mode type.
fn native_mode<T>(mode: u32) -> Result<T>
where
    T: TryFrom<u32>,
{
    T::try_from(mode).map_err(|_| {
        Error::new(
            ErrorKind::InvalidData,
            "source mode cannot be represented by the target platform",
        )
    })
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
