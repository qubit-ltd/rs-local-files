// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Ownership wrapper for a descriptor allocated by `GetSecurityInfo`.

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;

/// Owns a descriptor allocated by `GetSecurityInfo` and released by
/// `LocalFree`.
#[derive(Debug)]
#[must_use]
pub(super) struct LocalSecurityDescriptor(
    /// Pointer released with `LocalFree` when this wrapper is dropped.
    pub(super) PSECURITY_DESCRIPTOR,
);

impl Drop for LocalSecurityDescriptor {
    /// Releases the native allocation with the matching Windows allocator.
    fn drop(&mut self) {
        if !self.0.is_null() {
            // GetSecurityInfo documents LocalFree as the matching allocator.
            unsafe { LocalFree(self.0.cast()) };
        }
    }
}
