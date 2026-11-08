// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Ownership wrapper for a native Windows file handle.

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

/// Owns a kernel handle returned by `NtCreateFile`.
#[derive(Debug)]
#[must_use]
pub(super) struct NativeHandle(
    /// Kernel handle owned by this wrapper until it is transferred or dropped.
    pub(super) HANDLE,
);

impl Drop for NativeHandle {
    /// Closes the handle when it represents a valid owned kernel handle.
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // NtCreateFile returned an owned kernel handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}
