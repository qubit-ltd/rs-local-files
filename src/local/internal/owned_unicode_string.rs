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
pub(in crate::local) struct OwnedUnicodeString {
    /// Stable UTF-16 storage referenced by `header`.
    _units: Vec<u16>,
    /// NT string header passed to object attributes.
    header: UNICODE_STRING,
}

impl OwnedUnicodeString {
    /// Couples the UTF-16 storage with the header that borrows its buffer.
    #[must_use]
    #[inline(always)]
    pub(super) const fn new(units: Vec<u16>, header: UNICODE_STRING) -> Self {
        Self { _units: units, header }
    }

    /// Returns the stable header pointer while this owned string remains live.
    #[must_use]
    #[inline(always)]
    pub(super) fn header(&self) -> *const UNICODE_STRING {
        &raw const self.header
    }
}
