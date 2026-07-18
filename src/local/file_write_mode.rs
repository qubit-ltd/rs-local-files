// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! File write mode.

/// Mode used when opening a local file for writing.
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::FileWriteMode;
///
/// FileWriteMode::default();
/// ```
///
/// ```compile_fail
/// use qubit_local_files::FileWriteMode;
///
/// fn classify(mode: FileWriteMode) {
///     match mode {
///         FileWriteMode::OpenExistingAtStart => {}
///         FileWriteMode::CreateNew => {}
///         FileWriteMode::CreateOrTruncate => {}
///         FileWriteMode::AppendExisting => {}
///         FileWriteMode::AppendOrCreate => {}
///     }
/// }
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileWriteMode {
    /// Open an existing file for writing at offset zero without truncating it.
    OpenExistingAtStart,
    /// Create a new file and fail when the target already exists.
    CreateNew,
    /// Create a missing file or truncate an existing file.
    CreateOrTruncate,
    /// Append to an existing file and fail when the target is missing.
    AppendExisting,
    /// Append to an existing file or create it when missing.
    AppendOrCreate,
}

impl Default for FileWriteMode {
    /// Creates a missing file or truncates an existing file by default.
    #[inline]
    fn default() -> Self {
        Self::CreateOrTruncate
    }
}
