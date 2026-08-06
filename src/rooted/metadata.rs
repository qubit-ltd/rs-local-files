// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative entry metadata.
// qubit-style: allow source-test-pair

use std::fs;
#[cfg(unix)]
use std::ops::BitAnd;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::time::SystemTime;
#[cfg(unix)]
use std::time::{
    Duration,
    UNIX_EPOCH,
};

use super::{
    EntryKind,
    Permissions,
};

/// Metadata observed through an opened rooted directory authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct Metadata {
    /// The observed entry type.
    kind: EntryKind,
    /// The observed byte length when the platform reports one.
    len: u64,
    /// Last access time reported by the operating system.
    accessed_at: Option<SystemTime>,
    /// Last modification time reported by the operating system.
    modified_at: Option<SystemTime>,
    /// Creation time reported by the operating system when available.
    created_at: Option<SystemTime>,
    /// Cross-platform permissions observed through the open handle.
    permissions: Permissions,
    /// Native device identity when available.
    device_id: Option<u64>,
    /// Native file identity within its device when available.
    file_id: Option<u64>,
}

impl Metadata {
    /// Coverage-only construction from native metadata.
    #[cfg(all(coverage, unix))]
    pub fn coverage_from_native(metadata: &fs::Metadata) -> Self {
        Self::from_native(metadata)
    }

    /// Coverage-only construction from a Unix `stat` value.
    #[cfg(all(coverage, unix))]
    pub fn coverage_from_stat(status: &libc::stat) -> Self {
        Self::from_stat(status)
    }

    /// Builds rooted metadata from an opened native descriptor.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Native metadata obtained from the already-opened
    ///   descriptor.
    ///
    /// # Returns
    /// Rooted metadata preserving the descriptor-observed entry type and size.
    #[cfg(unix)]
    pub(crate) fn from_native(metadata: &fs::Metadata) -> Self {
        let kind = entry_kind_from_mode(metadata.mode());
        Self {
            kind,
            len: metadata.len(),
            accessed_at: metadata.accessed().ok(),
            modified_at: metadata.modified().ok(),
            created_at: metadata.created().ok(),
            permissions: Permissions::from_unix_mode(metadata.mode()),
            device_id: Some(metadata.dev()),
            file_id: Some(metadata.ino()),
        }
    }

    /// Builds rooted metadata from an opened native file handle.
    ///
    /// # Parameters
    ///
    /// * `file` - Already-opened native file handle.
    ///
    /// # Returns
    ///
    /// Rooted metadata preserving handle-observed type, timestamps, size, and
    /// native identity when the filesystem reports it.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when handle metadata cannot be inspected.
    pub(crate) fn from_open_file(file: &fs::File) -> std::io::Result<Self> {
        let metadata = file.metadata()?;
        #[cfg(unix)]
        {
            Ok(Self::from_native(&metadata))
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION,
                GetFileInformationByHandle,
            };

            let mut identity = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: `file` owns a live handle and `identity` is a correctly
            // sized writable buffer for `GetFileInformationByHandle`.
            let result = unsafe {
                GetFileInformationByHandle(
                    file.as_raw_handle(),
                    &raw mut identity,
                )
            };
            if result == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let file_id = (u64::from(identity.nFileIndexHigh) << 32)
                | u64::from(identity.nFileIndexLow);
            Ok(Self::from_windows_metadata(
                &metadata,
                Some(u64::from(identity.dwVolumeSerialNumber)),
                Some(file_id),
            ))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = metadata;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "rooted metadata is unsupported on this target",
            ))
        }
    }

    /// Builds rooted metadata from a Unix `fstatat` result.
    ///
    /// # Parameters
    ///
    /// * `status` - Fully initialized metadata returned with
    ///   `AT_SYMLINK_NOFOLLOW`.
    ///
    /// # Returns
    /// Rooted metadata for the final entry represented by `status`.
    #[cfg(unix)]
    pub(crate) fn from_stat(status: &libc::stat) -> Self {
        let kind = entry_kind_from_mode(status.st_mode);
        let (accessed_at, modified_at, created_at) = stat_times(status);
        Self {
            kind,
            len: u64::try_from(status.st_size).unwrap_or_default(),
            accessed_at,
            modified_at,
            created_at,
            permissions: Permissions::from_unix_mode(permission_mode(
                status.st_mode,
            )),
            device_id: native_id(status.st_dev),
            file_id: native_id(status.st_ino),
        }
    }

    /// Builds rooted metadata from Windows metadata and handle identity.
    #[cfg(windows)]
    fn from_windows_metadata(
        metadata: &fs::Metadata,
        device_id: Option<u64>,
        file_id: Option<u64>,
    ) -> Self {
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if metadata.is_file() {
            EntryKind::File
        } else if metadata.is_dir() {
            EntryKind::Directory
        } else {
            EntryKind::Other
        };
        Self {
            kind,
            len: metadata.len(),
            accessed_at: metadata.accessed().ok(),
            modified_at: metadata.modified().ok(),
            created_at: metadata.created().ok(),
            permissions: Permissions::from_read_only(
                metadata.permissions().readonly(),
            ),
            device_id,
            file_id,
        }
    }

    /// Returns the final entry type observed by the rooted operation.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    /// Returns the byte size reported by the rooted metadata operation.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn size(&self) -> u64 {
        self.len
    }

    /// Returns the last access time, or `None` when the platform did not
    /// provide one.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn accessed_at(&self) -> Option<SystemTime> {
        self.accessed_at
    }

    /// Returns the last modification time, or `None` when the platform did not
    /// provide one.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn modified_at(&self) -> Option<SystemTime> {
        self.modified_at
    }

    /// Returns the creation time, or `None` when the platform did not provide
    /// one.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn created_at(&self) -> Option<SystemTime> {
        self.created_at
    }

    /// Returns the permissions observed through the rooted operation.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn permissions(&self) -> Permissions {
        self.permissions
    }

    /// Returns whether two metadata values identify the same native entry.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn is_same_file(&self, other: &Self) -> bool {
        matches!(
            (self.device_id, self.file_id, other.device_id, other.file_id),
            (Some(left_device), Some(left_file), Some(right_device), Some(right_file))
                if left_device == right_device && left_file == right_file
        )
    }
}

/// Converts a platform-native mode into portable permission bits.
#[cfg(unix)]
#[must_use]
#[inline(always)]
fn permission_mode<T>(mode: T) -> u32
where
    T: Into<u32>,
{
    mode.into() & 0o7777
}

/// Converts a platform-native identity field into the portable representation.
#[cfg(unix)]
#[inline(always)]
fn native_id<T>(value: T) -> Option<u64>
where
    T: TryInto<u64>,
{
    value.try_into().ok()
}

/// Classifies one platform-native `st_mode` value.
#[cfg(unix)]
#[inline]
fn entry_kind_from_mode<T>(mode: T) -> EntryKind
where
    T: BitAnd<Output = T> + Copy + From<libc::mode_t> + PartialEq,
{
    let file_type = mode & T::from(libc::S_IFMT);
    if file_type == T::from(libc::S_IFREG) {
        EntryKind::File
    } else if file_type == T::from(libc::S_IFDIR) {
        EntryKind::Directory
    } else if file_type == T::from(libc::S_IFLNK) {
        EntryKind::Symlink
    } else if file_type == T::from(libc::S_IFIFO) {
        EntryKind::Fifo
    } else if file_type == T::from(libc::S_IFSOCK) {
        EntryKind::Socket
    } else if file_type == T::from(libc::S_IFBLK) {
        EntryKind::BlockDevice
    } else if file_type == T::from(libc::S_IFCHR) {
        EntryKind::CharDevice
    } else {
        EntryKind::Other
    }
}

/// Classifies Unix mode values for coverage tests without exposing the native
/// helper as part of the normal API.
#[cfg(all(coverage, unix))]
pub fn coverage_entry_kind_from_mode(mode: libc::mode_t) -> EntryKind {
    entry_kind_from_mode(mode)
}

/// Converts a non-negative Unix timestamp into [`SystemTime`].
///
/// Returns `None` for negative components or overflow.
#[cfg(unix)]
#[inline]
fn system_time<N>(seconds: libc::time_t, nanoseconds: N) -> Option<SystemTime>
where
    N: TryInto<u64>,
{
    let seconds = u64::try_from(seconds).ok()?;
    let nanoseconds = nanoseconds.try_into().ok()?;
    UNIX_EPOCH.checked_add(
        Duration::from_secs(seconds)
            .saturating_add(Duration::from_nanos(nanoseconds)),
    )
}

/// Extracts portable timestamps from Linux and Android `stat` values.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[inline]
fn stat_times(
    status: &libc::stat,
) -> (Option<SystemTime>, Option<SystemTime>, Option<SystemTime>) {
    (
        system_time(status.st_atime, status.st_atime_nsec),
        system_time(status.st_mtime, status.st_mtime_nsec),
        None,
    )
}

/// Extracts portable timestamps from Apple and FreeBSD `stat` values.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
#[inline]
fn stat_times(
    status: &libc::stat,
) -> (Option<SystemTime>, Option<SystemTime>, Option<SystemTime>) {
    (
        system_time(status.st_atime, status.st_atime_nsec),
        system_time(status.st_mtime, status.st_mtime_nsec),
        system_time(status.st_birthtime, status.st_birthtime_nsec),
    )
}

/// Reports unavailable timestamps on other Unix targets whose `stat` layouts
/// are not part of the crate's portable contract.
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
    ))
))]
#[inline]
fn stat_times(
    _status: &libc::stat,
) -> (Option<SystemTime>, Option<SystemTime>, Option<SystemTime>) {
    (None, None, None)
}
