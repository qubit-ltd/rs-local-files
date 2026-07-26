// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative entry metadata.

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::ops::BitAnd;
use std::time::SystemTime;
#[cfg(unix)]
use std::time::{
    Duration,
    UNIX_EPOCH,
};

use super::EntryKind;

/// Metadata observed through an opened rooted directory authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
}

impl Metadata {
    /// Builds rooted metadata from an opened native descriptor.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Native metadata obtained from the already-opened
    ///   descriptor.
    ///
    /// # Returns
    /// Rooted metadata preserving the descriptor-observed entry type and size.
    #[must_use]
    #[cfg(unix)]
    pub(crate) fn from_native(metadata: &fs::Metadata) -> Self {
        Self {
            kind: EntryKind::Directory,
            len: metadata.len(),
            accessed_at: metadata.accessed().ok(),
            modified_at: metadata.modified().ok(),
            created_at: metadata.created().ok(),
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
    #[must_use]
    pub(crate) fn from_stat(status: &libc::stat) -> Self {
        let kind = entry_kind_from_mode(status.st_mode);
        let (accessed_at, modified_at, created_at) = stat_times(status);
        Self {
            kind,
            len: u64::try_from(status.st_size).unwrap_or_default(),
            accessed_at,
            modified_at,
            created_at,
        }
    }

    /// Returns the final entry type observed by the rooted operation.
    #[must_use]
    #[inline(always)]
    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    /// Returns the byte size reported by the rooted metadata operation.
    #[must_use]
    #[inline(always)]
    pub const fn size(&self) -> u64 {
        self.len
    }

    /// Returns the last access time, or `None` when the platform did not
    /// provide one.
    #[must_use]
    #[inline(always)]
    pub const fn accessed_at(&self) -> Option<SystemTime> {
        self.accessed_at
    }

    /// Returns the last modification time, or `None` when the platform did not
    /// provide one.
    #[must_use]
    #[inline(always)]
    pub const fn modified_at(&self) -> Option<SystemTime> {
        self.modified_at
    }

    /// Returns the creation time, or `None` when the platform did not provide
    /// one.
    #[must_use]
    #[inline(always)]
    pub const fn created_at(&self) -> Option<SystemTime> {
        self.created_at
    }
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
    } else {
        EntryKind::Other
    }
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
