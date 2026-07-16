// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native Windows file-disposition request.

/// Native Windows request that marks a handle's object for deletion.
#[cfg(windows)]
#[repr(C)]
pub(super) struct FileDispositionInfo {
    /// Windows `BOOLEAN` value indicating whether deletion is requested.
    pub(super) delete_file: u8,
}
