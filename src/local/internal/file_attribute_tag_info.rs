// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Native Windows file attributes and reparse-tag information.

/// Native Windows attributes returned for a handle and its reparse tag.
#[cfg(windows)]
#[repr(C)]
pub(in crate::local) struct FileAttributeTagInfo {
    /// Bit mask of `FILE_ATTRIBUTE_*` values.
    pub(in crate::local) file_attributes: u32,
    /// Reparse tag, or zero when the object is not a reparse point.
    pub(in crate::local) reparse_tag: u32,
}
