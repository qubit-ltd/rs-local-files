// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cross-platform rooted entry permissions.

/// Permissions observed or applied through a rooted filesystem capability.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Permissions {
    /// Whether write access is disabled by the portable permission view.
    read_only: bool,
    /// Exact Unix permission bits when supplied or observed on Unix.
    unix_mode: Option<u32>,
}
#[allow(dead_code)]
impl Permissions {
    /// Creates a portable read-only or writable permission value.
    ///
    /// # Parameters
    ///
    /// - `read_only`: whether the portable permission view disables writing.
    ///
    /// # Returns
    ///
    /// A permission value with no explicit Unix mode.
    #[inline]
    pub const fn from_read_only(read_only: bool) -> Self {
        Self {
            read_only,
            unix_mode: None,
        }
    }

    /// Creates permissions from Unix mode bits.
    ///
    /// Bits outside the portable permission and special-bit range are ignored.
    ///
    /// # Parameters
    ///
    /// - `mode`: Unix permission and special bits. Bits outside `0o7777` are
    ///   ignored.
    ///
    /// # Returns
    ///
    /// A permission value containing the masked Unix mode bits.
    #[inline]
    pub const fn from_unix_mode(mode: u32) -> Self {
        let mode = mode & 0o7777;
        Self {
            read_only: mode & 0o222 == 0,
            unix_mode: Some(mode),
        }
    }

    /// Returns whether the portable permission view disables writing.
    ///
    /// # Returns
    ///
    /// `true` when write access is disabled; otherwise `false`.
    #[must_use]
    #[inline]
    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// Returns exact Unix mode bits, or `None` when created from a portable
    /// read-only value without an explicit Unix mode.
    ///
    /// # Returns
    ///
    /// `Some(mode)` contains the explicit Unix bits; `None` means this value
    /// was created from only a portable read-only setting.
    #[inline]
    pub const fn unix_mode(self) -> Option<u32> {
        self.unix_mode
    }

    /// Resolves a portable value against an existing Unix mode.
    ///
    /// An explicit Unix mode replaces `current_mode`. Otherwise read-only
    /// clears all write bits, while writable adds owner-write permission and
    /// preserves the other bits.
    ///
    /// # Parameters
    ///
    /// - `current_mode`: existing Unix mode bits used when no explicit mode is
    ///   stored.
    ///
    /// # Returns
    ///
    /// The explicit mode when present; otherwise `current_mode` with write bits
    /// cleared or owner-write added according to the portable setting.
    #[cfg(unix)]
    #[must_use]
    #[inline]
    pub const fn resolve_unix_mode(self, current_mode: u32) -> u32 {
        match self.unix_mode {
            Some(mode) => mode,
            None if self.read_only => current_mode & !0o222,
            None => current_mode | 0o200,
        }
    }
}
