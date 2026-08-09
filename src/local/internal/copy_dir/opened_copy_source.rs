// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Handle-authoritative regular-file opening for recursive copy.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs::File;
use std::fs::Metadata;
use std::fs::OpenOptions;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;

use crate::LocalSymlinkPolicy;
#[cfg(unix)]
use crate::local::internal::clear_nonblocking;
#[cfg(unix)]
use crate::local::internal::open_with_nonblocking_retry;

/// Open regular-file source and metadata read from the same handle.
#[must_use = "the opened source handle and its authoritative metadata must be consumed together"]
pub(super) struct OpenedCopySource {
    /// Open source file used for byte copying.
    file: File,
    /// Metadata loaded from the open source handle.
    metadata: Metadata,
}

impl OpenedCopySource {
    /// Opens a regular-file copy source according to the symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// * `path` - Source entry to open.
    /// * `symlink_policy` - Symbolic-link policy for the final component.
    ///
    /// # Returns
    ///
    /// An open regular-file handle and metadata for that exact handle.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for a symbolic link that must not be followed or
    /// for an opened non-regular resource. Other open and metadata errors are
    /// returned unchanged.
    #[inline(always)]
    pub(super) fn open(
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
        open_retry_timeout: Option<Duration>,
    ) -> Result<Self> {
        open_copy_source(path, symlink_policy, open_retry_timeout)
    }

    /// Splits this source into its open handle and authoritative metadata.
    ///
    /// # Returns
    ///
    /// Open source file followed by metadata read from that handle.
    #[must_use = "the opened source handle and authoritative metadata must both be retained"]
    #[inline(always)]
    pub(super) fn into_parts(self) -> (File, Metadata) {
        (self.file, self.metadata)
    }
}

/// Opens a Unix source without blocking on special files or following a
/// forbidden final symbolic link.
#[cfg(unix)]
fn open_copy_source(
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    open_retry_timeout: Option<Duration>,
) -> Result<OpenedCopySource> {
    let mut options = OpenOptions::new();
    let mut flags = libc::O_NONBLOCK;
    if !symlink_policy.follows() {
        flags |= libc::O_NOFOLLOW;
    }
    options.read(true).custom_flags(flags);
    let file = open_with_nonblocking_retry(open_retry_timeout, || options.open(path))
        .map_err(|error| normalize_unix_source_open_error(path, error))?;
    let metadata = file.metadata()?;
    reject_non_regular_source(path, &metadata)?;
    clear_nonblocking(file.as_raw_fd())?;
    Ok(OpenedCopySource { file, metadata })
}

/// Normalizes final-link and non-openable special-file errors.
#[cfg(unix)]
#[inline]
fn normalize_unix_source_open_error(path: &Path, error: Error) -> Error {
    match error.raw_os_error() {
        Some(libc::ELOOP | libc::ENXIO | libc::ENODEV) => invalid_copy_source(path),
        _ => error,
    }
}

/// Opens a Windows source and rejects name-surrogate reparse points when links
/// are disabled.
#[cfg(windows)]
fn open_copy_source(
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    _open_retry_timeout: Option<Duration>,
) -> Result<OpenedCopySource> {
    const IO_REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;

    let mut options = OpenOptions::new();
    options.read(true);
    if !symlink_policy.follows() {
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let mut tag_info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the raw handle is live for the call, `tag_info` is a correctly
    // sized writable output buffer, and the API does not retain either value.
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut tag_info).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(Error::last_os_error());
    }
    if !symlink_policy.follows() && tag_info.ReparseTag & IO_REPARSE_TAG_NAME_SURROGATE != 0 {
        return Err(invalid_copy_source(path));
    }
    let metadata = file.metadata()?;
    reject_non_regular_source(path, &metadata)?;
    Ok(OpenedCopySource { file, metadata })
}

/// Opens a regular source on targets without specialized open flags.
#[cfg(not(any(unix, windows)))]
fn open_copy_source(
    path: &Path,
    _symlink_policy: LocalSymlinkPolicy,
    _open_retry_timeout: Option<Duration>,
) -> Result<OpenedCopySource> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    reject_non_regular_source(path, &metadata)?;
    Ok(OpenedCopySource { file, metadata })
}

/// Rejects metadata that does not represent a regular file.
#[inline]
fn reject_non_regular_source(path: &Path, metadata: &Metadata) -> Result<()> {
    if metadata.is_file() {
        Ok(())
    } else {
        Err(invalid_copy_source(path))
    }
}

/// Creates the stable error used for a non-regular copy source.
#[must_use]
#[inline]
fn invalid_copy_source(path: &Path) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("copy source is not a regular file: {}", path.display()),
    )
}
