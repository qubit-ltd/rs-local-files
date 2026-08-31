// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owned UTF-16 storage for Windows counted Unicode strings.

use windows_sys::Win32::Foundation::UNICODE_STRING;

/// Owns UTF-16 storage and its borrowed `UNICODE_STRING` header.
pub(super) struct OwnedUnicodeString {
    /// Stable UTF-16 storage referenced by `header`.
    pub(super) _units: Vec<u16>,
    /// NT string header passed to object attributes.
    pub(super) header: UNICODE_STRING,
}
