// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cross-platform rooted entry permissions.
// qubit-style: allow source-test-pair

/// Permissions observed or applied through a rooted filesystem capability.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Permissions {
    /// Whether write access is disabled by the portable permission view.
    read_only: bool,
    /// Exact Unix permission bits when supplied or observed on Unix.
    unix_mode: Option<u32>,
}

#[allow(dead_code)]
impl Permissions {
    /// Test-support-only access to rooted permission resolution.
    /// Creates a portable read-only or writable permission value.
    pub(crate) const fn from_read_only(read_only: bool) -> Self {
        Self {
            read_only,
            unix_mode: None,
        }
    }

    /// Creates permissions from Unix mode bits.
    ///
    /// Bits outside the portable permission and special-bit range are ignored.
    pub(crate) const fn from_unix_mode(mode: u32) -> Self {
        let mode = mode & 0o7777;
        Self {
            read_only: mode & 0o222 == 0,
            unix_mode: Some(mode),
        }
    }

    /// Returns whether the portable permission view disables writing.
    pub(crate) const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// Returns exact Unix mode bits when they are available.
    pub(crate) const fn unix_mode(self) -> Option<u32> {
        self.unix_mode
    }

    /// Resolves a portable value against an existing Unix mode.
    #[cfg(unix)]
    pub(crate) const fn resolve_unix_mode(self, current_mode: u32) -> u32 {
        match self.unix_mode {
            Some(mode) => mode,
            None if self.read_only => current_mode & !0o222,
            None => current_mode | 0o200,
        }
    }
}
