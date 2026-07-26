// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative entry metadata.

use std::fs;

/// The type of a rooted filesystem entry observed without following its final
/// symbolic link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
    /// A platform-specific special entry.
    Other,
}

/// Metadata observed through an opened rooted directory authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    /// The observed entry type.
    kind: EntryKind,
    /// The observed byte length when the platform reports one.
    len: u64,
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
    pub(crate) fn from_native(metadata: &fs::Metadata) -> Self {
        let file_type = metadata.file_type();
        let kind = if file_type.is_file() {
            EntryKind::File
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        };
        Self {
            kind,
            len: metadata.len(),
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
        let kind = match status.st_mode & libc::S_IFMT {
            libc::S_IFREG => EntryKind::File,
            libc::S_IFDIR => EntryKind::Directory,
            libc::S_IFLNK => EntryKind::Symlink,
            _ => EntryKind::Other,
        };
        Self {
            kind,
            len: u64::try_from(status.st_size).unwrap_or_default(),
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
}
