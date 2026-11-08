// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Ownership wrapper for a Windows process-token handle.

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

/// Owns a process-token handle returned by `OpenProcessToken`.
#[derive(Debug)]
#[must_use]
pub(super) struct Token(
    /// Process-token handle owned by this wrapper until it is dropped.
    pub(super) HANDLE,
);

impl Drop for Token {
    /// Closes the owned handle when it is valid.
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // The token handle was returned by OpenProcessToken.
            unsafe { CloseHandle(self.0) };
        }
    }
}
