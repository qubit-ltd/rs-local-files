// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Redacted debug formatting for temporary-directory defaults.

use std::fmt::Debug;
use std::fmt::Formatter;
use std::fmt::Result;

use crate::LocalTempDirectoryOptions;

/// Formats temporary-directory defaults without exposing their parent path.
pub(in crate::file_system) struct TempDirectoryDebug<'a>(
    /// The options whose diagnostic representation must redact the parent path.
    pub(in crate::file_system) &'a LocalTempDirectoryOptions,
);

impl Debug for TempDirectoryDebug<'_> {
    /// Writes the option values while replacing the parent path with its
    /// redacted form.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter that receives the redacted debug representation.
    ///
    /// # Returns
    ///
    /// Returns an error if writing the debug representation fails.
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("LocalTempDirectoryOptions")
            .field("parent", &self.0.parent().map(crate::local_file_system::redacted_path))
            .field("prefix", &self.0.prefix())
            .field("suffix", &self.0.suffix())
            .field("max_attempts", &self.0.max_attempts())
            .field("create_parent", &self.0.creates_parent())
            .field("privacy", &self.0.privacy())
            .field("cleanup_limits", &self.0.cleanup_limits())
            .field("owner_access_policy", &self.0.owner_access_policy())
            .finish()
    }
}
