// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable directory identities for cycle-safe filesystem traversal.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs::Metadata;
use std::path::Path;

#[cfg(not(unix))]
use std::path::PathBuf;

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
};

/// Identifies one directory independently of the path used to reach it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum DirectoryIdentity {
    /// Platform-native filesystem and file identifiers.
    #[cfg(any(unix, windows))]
    Native {
        /// Filesystem or volume identifier containing the directory.
        filesystem: u64,
        /// File identifier within `filesystem`.
        file: u64,
    },
    /// Canonical-path fallback when native identity is unavailable.
    #[cfg(not(unix))]
    Canonical(PathBuf),
}

impl DirectoryIdentity {
    /// Builds a directory identity from metadata and its canonical path.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Metadata for the directory being identified.
    /// * `canonical_path` - Canonical path used only when native identity is
    ///   unavailable.
    ///
    /// # Returns
    ///
    /// A stable identity suitable for active-ancestor cycle detection.
    #[cfg(unix)]
    #[inline]
    pub(crate) fn from_metadata(metadata: &Metadata, canonical_path: &Path) -> Self {
        #[cfg(feature = "internal-test-support")]
        if injected_cycle_identity() {
            return Self::Native {
                filesystem: 0,
                file: 0,
            };
        }
        let _ = canonical_path;
        Self::Native {
            filesystem: metadata.dev(),
            file: metadata.ino(),
        }
    }

    /// Builds a directory identity from metadata and its canonical path.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Metadata for the directory being identified.
    /// * `canonical_path` - Canonical fallback path.
    ///
    /// # Returns
    ///
    /// A native Windows identity when available, otherwise the canonical path.
    #[cfg(windows)]
    pub(crate) fn from_metadata(metadata: &Metadata, canonical_path: &Path) -> Self {
        #[cfg(feature = "internal-test-support")]
        if injected_cycle_identity() {
            return Self::Native {
                filesystem: 0,
                file: 0,
            };
        }
        let _ = metadata;
        windows_native_identity(canonical_path)
            .unwrap_or_else(|| Self::Canonical(canonical_path.to_path_buf()))
    }

    /// Builds a canonical directory identity on other targets.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Metadata retained for signature parity.
    /// * `canonical_path` - Canonical directory path.
    ///
    /// # Returns
    ///
    /// The canonical path wrapped as a directory identity.
    #[cfg(not(any(unix, windows)))]
    #[inline(always)]
    pub(crate) fn from_metadata(metadata: &Metadata, canonical_path: &Path) -> Self {
        let _ = metadata;
        Self::Canonical(canonical_path.to_path_buf())
    }
}

/// Reads a native Windows directory identity through an open handle.
///
/// # Parameters
///
/// * `path` - Canonical directory path to open.
///
/// # Returns
///
/// The volume and file identifiers, or `None` when the native query is not
/// available for this directory.
#[cfg(windows)]
fn windows_native_identity(path: &Path) -> Option<DirectoryIdentity> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `directory` owns a valid handle for the duration of the call,
    // and `information` is a writable structure with the required layout.
    let result =
        unsafe { GetFileInformationByHandle(directory.as_raw_handle(), &raw mut information) };
    if result == 0 {
        return None;
    }
    Some(DirectoryIdentity::Native {
        filesystem: u64::from(information.dwVolumeSerialNumber),
        file: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

/// Reports whether traversal identities should collide in a test-support
/// process.
///
/// # Returns
///
/// `true` when either directory-cycle fault is selected.
#[cfg(all(feature = "internal-test-support", any(unix, windows)))]
#[must_use]
#[inline]
fn injected_cycle_identity() -> bool {
    super::test_support::is_enabled("copy-dir-directory-identity-cycle")
        || super::test_support::is_enabled("dir-size-directory-identity-cycle")
        || super::test_support::is_enabled("walker-directory-identity-cycle")
}
