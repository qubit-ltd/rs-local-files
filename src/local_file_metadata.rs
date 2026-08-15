// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by metadata integration tests.

use std::fs::Metadata;
use std::time::SystemTime;

use crate::LocalFileKind;
use crate::LocalFilePermissions;

/// Normalized metadata for a native filesystem entry.
#[derive(Clone, Debug)]
#[must_use]
pub struct LocalFileMetadata {
    /// Normalized entry kind.
    kind: LocalFileKind,
    /// Native metadata length.
    len: u64,
    /// Last access time when exposed by the platform.
    accessed_at: Option<SystemTime>,
    /// Last modification time when exposed by the platform.
    modified_at: Option<SystemTime>,
    /// Creation time when exposed by the platform.
    created_at: Option<SystemTime>,
    /// Read-only and platform-specific mode observations.
    permissions: LocalFilePermissions,
}

impl LocalFileMetadata {
    /// Creates normalized metadata from platform-independent parts.
    ///
    /// # Parameters
    ///
    /// - `kind`: Normalized entry kind.
    /// - `len`: Native entry length.
    /// - `accessed_at`: Optional access time.
    /// - `modified_at`: Optional modification time.
    /// - `created_at`: Optional creation time.
    pub(crate) const fn from_parts(
        kind: LocalFileKind,
        len: u64,
        accessed_at: Option<SystemTime>,
        modified_at: Option<SystemTime>,
        created_at: Option<SystemTime>,
    ) -> Self {
        Self {
            kind,
            len,
            accessed_at,
            modified_at,
            created_at,
            permissions: LocalFilePermissions::new(false, None),
        }
    }

    /// Normalizes native metadata without following any additional path.
    ///
    /// # Parameters
    ///
    /// - `metadata`: Metadata already obtained using the caller's follow
    ///   policy.
    pub(crate) fn from_native(metadata: &Metadata) -> Self {
        let file_type = metadata.file_type();
        let kind = local_file_kind(file_type);
        let permissions = local_file_permissions(metadata);
        Self {
            kind,
            len: metadata.len(),
            accessed_at: metadata.accessed().ok(),
            modified_at: metadata.modified().ok(),
            created_at: metadata.created().ok(),
            permissions,
        }
    }

    /// Returns the normalized entry kind.
    pub const fn kind(&self) -> LocalFileKind {
        self.kind
    }

    /// Returns the native metadata length in bytes.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Reports whether the entry length is zero.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the access time, or `None` when unavailable.
    #[must_use]
    pub const fn accessed_at(&self) -> Option<SystemTime> {
        self.accessed_at
    }

    /// Returns the modification time, or `None` when unavailable.
    #[must_use]
    pub const fn modified_at(&self) -> Option<SystemTime> {
        self.modified_at
    }

    /// Returns the creation time, or `None` when unavailable.
    #[must_use]
    pub const fn created_at(&self) -> Option<SystemTime> {
        self.created_at
    }

    /// Returns permissions observed with this metadata value.
    pub const fn permissions(&self) -> LocalFilePermissions {
        self.permissions
    }
}

#[cfg(unix)]
fn local_file_permissions(metadata: &Metadata) -> LocalFilePermissions {
    use std::os::unix::fs::MetadataExt;

    LocalFilePermissions::new(
        metadata.permissions().readonly(),
        Some(metadata.mode() & 0o7777),
    )
}

#[cfg(not(unix))]
fn local_file_permissions(metadata: &Metadata) -> LocalFilePermissions {
    LocalFilePermissions::new(metadata.permissions().readonly(), None)
}

/// Classifies a native file type without following its final path component.
///
/// # Parameters
///
/// - `file_type`: Native type bits observed for the final entry.
///
/// # Returns
///
/// The most specific platform-independent kind available for the entry.
#[cfg(unix)]
#[inline]
fn local_file_kind(file_type: std::fs::FileType) -> LocalFileKind {
    use std::os::unix::fs::FileTypeExt;

    if file_type.is_file() {
        LocalFileKind::File
    } else if file_type.is_dir() {
        LocalFileKind::Directory
    } else if file_type.is_symlink() {
        LocalFileKind::Symlink
    } else if file_type.is_fifo() {
        LocalFileKind::Fifo
    } else if file_type.is_socket() {
        LocalFileKind::Socket
    } else if file_type.is_block_device() {
        LocalFileKind::BlockDevice
    } else if file_type.is_char_device() {
        LocalFileKind::CharDevice
    } else {
        LocalFileKind::Other
    }
}

/// Classifies a native file type on platforms without portable special-entry
/// predicates.
///
/// # Parameters
///
/// - `file_type`: Native type bits observed for the final entry.
///
/// # Returns
///
/// The regular, directory, symlink, or fallback kind available on the target.
#[cfg(not(unix))]
#[inline]
fn local_file_kind(file_type: std::fs::FileType) -> LocalFileKind {
    if file_type.is_file() {
        LocalFileKind::File
    } else if file_type.is_dir() {
        LocalFileKind::Directory
    } else if file_type.is_symlink() {
        LocalFileKind::Symlink
    } else {
        LocalFileKind::Other
    }
}
