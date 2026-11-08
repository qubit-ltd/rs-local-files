// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Redacted debug formatting for temporary-file defaults.

use std::fmt::Debug;
use std::fmt::Formatter;
use std::fmt::Result;

use crate::LocalTempFileOptions;

/// Formats temporary-file defaults without exposing their parent path.
pub(in crate::file_system) struct TempFileDebug<'a>(
    /// The options whose diagnostic representation must redact the parent path.
    pub(in crate::file_system) &'a LocalTempFileOptions,
);

impl Debug for TempFileDebug<'_> {
    /// Writes the option values while replacing the parent path with its
    /// redacted form.
    ///
    /// # Parameters
    ///
    /// * `f` is the formatter that receives the debug representation.
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("LocalTempFileOptions")
            .field("parent", &self.0.parent().map(crate::local_file_system::redacted_path))
            .field("prefix", &self.0.prefix())
            .field("suffix", &self.0.suffix())
            .field("max_attempts", &self.0.max_attempts())
            .field("create_parent", &self.0.creates_parent())
            .field("privacy", &self.0.privacy())
            .finish()
    }
}
