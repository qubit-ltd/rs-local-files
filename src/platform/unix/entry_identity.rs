// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable Unix entry identities.

use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;

use super::NamespaceHandle;
use crate::LocalResult;
use crate::RelativePath;

/// Identifies one Unix entry independently of the path used to reach it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub(crate) struct EntryIdentity {
    /// Device containing the entry.
    device: u64,
    /// Inode number within `device`.
    inode: u64,
}

impl EntryIdentity {
    /// Captures identity from metadata obtained through an open descriptor.
    ///
    /// # Parameters
    ///
    /// - `metadata`: Metadata for the already-opened entry.
    pub(super) fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    /// Captures identity from a completed no-follow `fstatat` result.
    ///
    /// # Parameters
    ///
    /// - `status`: Fully initialized native status for one entry.
    pub(super) fn from_stat(status: &libc::stat) -> Self {
        Self {
            device: native_identity_value(status.st_dev),
            inode: native_identity_value(status.st_ino),
        }
    }

    /// Reports whether `path` still names this native entry.
    ///
    /// # Parameters
    ///
    /// - `namespace`: Open authority used for the no-follow lookup.
    /// - `path`: Validated path relative to `namespace`.
    ///
    /// # Returns
    ///
    /// `true` only when the current device and inode match this identity.
    ///
    /// # Errors
    ///
    /// Returns the namespace lookup error when the path cannot be inspected.
    #[allow(dead_code)]
    pub(crate) fn matches_path(&self, namespace: &NamespaceHandle, path: &RelativePath) -> LocalResult<bool> {
        namespace.entry_identity(path).map(|current| current == *self)
    }
}

/// Normalizes a platform-native non-negative identity component.
fn native_identity_value<T>(value: T) -> u64
where
    T: TryInto<u64>,
{
    value.try_into().unwrap_or_default()
}
