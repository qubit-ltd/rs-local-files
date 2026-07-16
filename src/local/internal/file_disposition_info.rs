// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Native Windows file-disposition request.

/// Native Windows request that marks a handle's object for deletion.
#[cfg(windows)]
#[repr(C)]
pub(in crate::local) struct FileDispositionInfo {
    /// Windows `BOOLEAN` value indicating whether deletion is requested.
    pub(in crate::local) delete_file: u8,
}
