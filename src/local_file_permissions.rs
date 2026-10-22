// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Portable permission observations for native metadata.

/// Portable permission information observed for a local filesystem entry.
///
/// The read-only flag is available on every platform. Unix mode bits are
/// included only when the observing platform exposes them.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFilePermissions;
///
/// let permissions = LocalFilePermissions::new(true, Some(0o444));
/// assert!(permissions.is_read_only());
/// assert_eq!(permissions.unix_mode(), Some(0o444));
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFilePermissions {
    /// Portable read-only observation derived from native permissions.
    read_only: bool,
    /// Unix mode bits when the current platform exposes them.
    unix_mode: Option<u32>,
}

impl LocalFilePermissions {
    /// Creates a permission observation from platform-specific metadata.
    ///
    /// # Parameters
    ///
    /// - `read_only`: whether the native metadata marks the entry read-only.
    /// - `unix_mode`: Unix mode bits, or `None` when they are unavailable.
    #[inline]
    pub const fn new(read_only: bool, unix_mode: Option<u32>) -> Self {
        Self { read_only, unix_mode }
    }

    /// Reports whether the native entry is read-only.
    #[must_use]
    #[inline]
    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// Returns Unix mode bits when the platform exposes them.
    ///
    /// `None` means the observing platform does not expose Unix mode bits.
    #[inline]
    pub const fn unix_mode(self) -> Option<u32> {
        self.unix_mode
    }
}
