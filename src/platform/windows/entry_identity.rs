// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable Windows entry identities.

use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;

use super::NamespaceHandle;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::RelativePath;

/// Identifies one Windows entry by volume and file identifiers.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub(crate) struct EntryIdentity {
    /// Volume containing the entry.
    volume: u64,
    /// File identifier within `volume`.
    file: u64,
}

impl EntryIdentity {
    /// Captures identity from an already-opened native handle.
    ///
    /// # Errors
    ///
    /// Returns the native identity-query error.
    pub(super) fn from_file(file: &File) -> io::Result<Self> {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` owns a live handle and `information` is writable
        // storage with the exact layout expected by the API.
        let result = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            volume: u64::from(information.dwVolumeSerialNumber),
            file: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        })
    }

    /// Reports whether `path` still names this native entry.
    ///
    /// # Errors
    ///
    /// Returns the authority lookup error when the current entry identity
    /// cannot be observed.
    pub(crate) fn matches_path(&self, namespace: &NamespaceHandle, path: &RelativePath) -> LocalResult<bool> {
        namespace.entry_identity(path).map(|current| current == *self)
    }

    /// Converts an identity query into a structured metadata error.
    pub(super) fn for_file(file: &File, path: &RelativePath) -> LocalResult<Self> {
        Self::from_file(file).map_err(|error| {
            super::namespace_handle::io_error(LocalFileOperation::Metadata, path.as_path(), None, error)
        })
    }
}
