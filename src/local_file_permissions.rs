// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Portable permission observations for native metadata.

/// Permissions exposed by a local metadata observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFilePermissions {
    read_only: bool,
    unix_mode: Option<u32>,
}

impl LocalFilePermissions {
    /// Creates a permission observation.
    #[inline]
    pub const fn new(read_only: bool, unix_mode: Option<u32>) -> Self {
        Self { read_only, unix_mode }
    }

    /// Reports whether the native entry is read-only.
    #[must_use]
    #[inline(always)]
    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// Returns Unix mode bits when the platform exposes them.
    #[must_use]
    #[inline(always)]
    pub const fn unix_mode(self) -> Option<u32> {
        self.unix_mode
    }
}
