// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable native filesystem path limits.

use super::SizeLimit;

/// Native path limits observed for one filesystem authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileSystemLimits {
    max_path_bytes: SizeLimit,
    max_file_name_bytes: SizeLimit,
}

impl LocalFileSystemLimits {
    /// Creates limits from independently observed native dimensions.
    #[inline(always)]
    pub const fn new(
        max_path_bytes: SizeLimit,
        max_file_name_bytes: SizeLimit,
    ) -> Self {
        Self {
            max_path_bytes,
            max_file_name_bytes,
        }
    }

    /// Returns the maximum complete native path size in bytes.
    #[inline(always)]
    pub const fn max_path_bytes(&self) -> SizeLimit {
        self.max_path_bytes
    }

    /// Returns the maximum native file-name component size in bytes.
    #[inline(always)]
    pub const fn max_file_name_bytes(&self) -> SizeLimit {
        self.max_file_name_bytes
    }
}
