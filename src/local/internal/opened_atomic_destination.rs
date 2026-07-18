// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Open Unix atomic destinations and stable file identity.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::ffi::CString;
use std::fs::{
    self,
    File,
    OpenOptions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{
    MetadataExt,
    OpenOptionsExt,
};
use std::path::Path;
use std::time::Duration;

use super::rooted_file_io::open_file_at;
use super::unix_nonblocking::{
    clear_nonblocking,
    wait_for_nonblocking_open_retry,
};
use super::unix_stat::is_regular_file_mode;

/// Open destination handle and Unix identity used by atomic replacement.
#[must_use = "the destination handle and captured identity must remain authoritative until commit"]
pub(crate) struct OpenedAtomicDestination {
    /// Open destination handle supplying commit-time metadata.
    file: File,
    /// Device identifier captured from the open handle.
    device: u64,
    /// Inode identifier captured from the open handle.
    inode: u64,
}

impl OpenedAtomicDestination {
    /// Constructs and validates destination identity from an open file.
    pub(crate) fn from_file(file: File) -> Result<Self> {
        let mut metadata_result = file.metadata();
        #[cfg(coverage)]
        if super::coverage_fault::is_enabled("atomic-destination-stat") {
            metadata_result = Err(Error::from_raw_os_error(libc::EIO));
        }
        let metadata = metadata_result?;
        if !metadata.is_file()
            || coverage_fault_enabled("atomic-destination-type")
        {
            return Err(invalid_atomic_destination());
        }
        clear_nonblocking(file.as_raw_fd())?;
        Ok(Self {
            file,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    /// Returns the open destination handle.
    #[must_use]
    #[inline(always)]
    pub(crate) fn file(&self) -> &File {
        &self.file
    }

    /// Returns the captured device identifier.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn device(&self) -> u64 {
        self.device
    }

    /// Returns the captured inode identifier.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn inode(&self) -> u64 {
        self.inode
    }
}

/// Opens the current destination without following its final component.
pub(crate) fn open_atomic_destination(
    path: &Path,
) -> Result<Option<OpenedAtomicDestination>> {
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("atomic-destination-open") {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut retry_delay = Duration::ZERO;
    let file = loop {
        let mut result = options.open(path);
        #[cfg(coverage)]
        if super::coverage_fault::take("atomic-destination-would-block") {
            result = Err(Error::from(ErrorKind::WouldBlock));
        } else if super::coverage_fault::is_enabled(
            "atomic-destination-invalid",
        ) {
            result = Err(Error::from_raw_os_error(libc::ELOOP));
        } else if super::coverage_fault::is_enabled("atomic-destination-native")
        {
            result = Err(Error::from_raw_os_error(libc::EIO));
        }
        match result {
            Ok(file) => break file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                wait_for_nonblocking_open_retry(&mut retry_delay);
            }
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ELOOP | libc::ENXIO | libc::ENODEV)
                ) =>
            {
                return Err(invalid_atomic_destination());
            }
            Err(error) => return Err(error),
        }
    };
    OpenedAtomicDestination::from_file(file).map(Some)
}

/// Checks whether a path still names the opened destination identity.
pub(crate) fn destination_identity_matches(
    path: &Path,
    destination: &OpenedAtomicDestination,
) -> Result<bool> {
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("atomic-identity-mismatch") {
        return Ok(false);
    }
    let mut result = fs::symlink_metadata(path);
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("atomic-identity-missing") {
        result = Err(Error::from(ErrorKind::NotFound));
    } else if super::coverage_fault::is_enabled("atomic-identity-inspect") {
        result = Err(Error::from_raw_os_error(libc::EIO));
    }
    match result {
        Ok(metadata) => Ok(metadata.file_type().is_file()
            && metadata.dev() == destination.device
            && metadata.ino() == destination.inode),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Opens the current rooted destination without following its final entry.
pub(in crate::local) fn open_rooted_atomic_destination(
    parent: &File,
    name: &CString,
) -> Result<Option<OpenedAtomicDestination>> {
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("rooted-destination-open") {
        return Err(Error::from_raw_os_error(libc::EIO));
    } else if super::coverage_fault::is_enabled("rooted-destination-missing") {
        return Ok(None);
    }
    let flags =
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    let mut retry_delay = Duration::ZERO;
    let file = loop {
        let mut result = open_file_at(parent, name, flags, 0);
        #[cfg(coverage)]
        if super::coverage_fault::take("rooted-destination-would-block") {
            result = Err(Error::from(ErrorKind::WouldBlock));
        } else if super::coverage_fault::is_enabled(
            "rooted-destination-invalid",
        ) {
            result = Err(Error::from_raw_os_error(libc::ELOOP));
        } else if super::coverage_fault::is_enabled("rooted-destination-native")
        {
            result = Err(Error::from_raw_os_error(libc::EIO));
        }
        match result {
            Ok(file) => break file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                wait_for_nonblocking_open_retry(&mut retry_delay);
            }
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ELOOP | libc::ENXIO | libc::ENODEV)
                ) =>
            {
                return Err(invalid_atomic_destination());
            }
            Err(error) => return Err(error),
        }
    };
    OpenedAtomicDestination::from_file(file).map(Some)
}

/// Checks whether a rooted entry still names an opened destination identity.
pub(in crate::local) fn rooted_destination_identity_matches(
    parent: &File,
    name: &CString,
    destination: &OpenedAtomicDestination,
) -> Result<bool> {
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("rooted-identity-mismatch")
        || super::coverage_fault::is_enabled("rooted-identity-missing")
    {
        return Ok(false);
    } else if super::coverage_fault::is_enabled("rooted-identity-inspect") {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    let Some(status) = rooted_destination_status(parent, name)? else {
        return Ok(false);
    };
    if !is_regular_file_mode(status.st_mode)
        || coverage_fault_enabled("rooted-status-type")
    {
        return Ok(false);
    }
    let device = native_identity_component(status.st_dev)?;
    let inode = native_identity_component(status.st_ino)?;
    Ok(device == destination.device() && inode == destination.inode())
}

/// Reads rooted destination status without following the final entry.
fn rooted_destination_status(
    parent: &File,
    name: &CString,
) -> Result<Option<libc::stat>> {
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("rooted-status-missing") {
        return Ok(None);
    } else if super::coverage_fault::is_enabled("rooted-status-error") {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `status` is writable storage and the parent descriptor and name
    // remain live for this non-retaining lookup.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        let error = Error::last_os_error();
        return if error.kind() == ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    // SAFETY: successful `fstatat` initialized the complete status value.
    Ok(Some(unsafe { status.assume_init() }))
}

/// Converts a platform-native stat identity component to the public width.
fn native_identity_component<T>(value: T) -> Result<u64>
where
    u64: TryFrom<T>,
{
    #[cfg(coverage)]
    if super::coverage_fault::is_enabled("rooted-identity-overflow") {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "injected atomic destination identity overflow",
        ));
    }
    match u64::try_from(value) {
        Ok(value) => Ok(value),
        Err(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "atomic destination identity is outside the supported range",
        )),
    }
}

/// Returns whether a coverage-only atomic destination fault is selected.
fn coverage_fault_enabled(name: &str) -> bool {
    #[cfg(coverage)]
    return super::coverage_fault::is_enabled(name);
    #[cfg(not(coverage))]
    {
        let _ = name;
        false
    }
}

/// Creates the stable type error for atomic destinations.
pub(crate) fn invalid_atomic_destination() -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        "atomic write destination must be absent or a regular file",
    )
}
