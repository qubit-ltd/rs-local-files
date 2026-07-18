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

use std::fs::{
    File,
    Metadata,
    OpenOptions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::Path;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_OPEN_REPARSE_POINT,
    FileAttributeTagInfo,
    GetFileInformationByHandleEx,
};

#[cfg(unix)]
use crate::local::internal::{
    clear_nonblocking,
    wait_for_nonblocking_open_retry,
};

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
    /// * `follow_symlinks` - Whether the final symbolic link may be followed.
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
    pub(super) fn open(path: &Path, follow_symlinks: bool) -> Result<Self> {
        open_copy_source(path, follow_symlinks)
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
    follow_symlinks: bool,
) -> Result<OpenedCopySource> {
    let mut options = OpenOptions::new();
    let mut flags = libc::O_NONBLOCK;
    if !follow_symlinks {
        flags |= libc::O_NOFOLLOW;
    }
    options.read(true).custom_flags(flags);
    let file = open_unix_source(&options, path)
        .map_err(|error| normalize_unix_source_open_error(path, error))?;
    let metadata = file.metadata()?;
    reject_non_regular_source(path, &metadata)?;
    clear_nonblocking(file.as_raw_fd())?;
    Ok(OpenedCopySource { file, metadata })
}

/// Retries a nonblocking open while a regular-file lease is being broken.
///
/// Linux reports `WouldBlock` for `O_NONBLOCK` opens that conflict with an
/// active lease. Retrying preserves normal blocking-open semantics without
/// ever allowing a racing FIFO or device path to block the opening thread.
#[cfg(unix)]
fn open_unix_source(options: &OpenOptions, path: &Path) -> Result<File> {
    let mut retry_delay = Duration::ZERO;
    loop {
        match options.open(path) {
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                wait_for_nonblocking_open_retry(&mut retry_delay);
            }
            result => return result,
        }
    }
}

/// Normalizes final-link and non-openable special-file errors.
#[cfg(unix)]
fn normalize_unix_source_open_error(path: &Path, error: Error) -> Error {
    match error.raw_os_error() {
        Some(libc::ELOOP | libc::ENXIO | libc::ENODEV) => {
            invalid_copy_source(path)
        }
        _ => error,
    }
}

/// Opens a Windows source and rejects name-surrogate reparse points when links
/// are disabled.
#[cfg(windows)]
fn open_copy_source(
    path: &Path,
    follow_symlinks: bool,
) -> Result<OpenedCopySource> {
    const IO_REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;

    let mut options = OpenOptions::new();
    options.read(true);
    if !follow_symlinks {
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
    if !follow_symlinks
        && tag_info.ReparseTag & IO_REPARSE_TAG_NAME_SURROGATE != 0
    {
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
    _follow_symlinks: bool,
) -> Result<OpenedCopySource> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    reject_non_regular_source(path, &metadata)?;
    Ok(OpenedCopySource { file, metadata })
}

/// Rejects metadata that does not represent a regular file.
fn reject_non_regular_source(path: &Path, metadata: &Metadata) -> Result<()> {
    if metadata.is_file() {
        Ok(())
    } else {
        Err(invalid_copy_source(path))
    }
}

/// Creates the stable error used for a non-regular copy source.
fn invalid_copy_source(path: &Path) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("copy source is not a regular file: {}", path.display()),
    )
}
