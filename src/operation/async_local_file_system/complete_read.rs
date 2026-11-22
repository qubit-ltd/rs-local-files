// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Complete-read output and the path context retained for UTF-8 decoding.

use std::path::PathBuf;

/// Complete bytes and the virtual path binding used to read them.
///
/// The path and current-directory snapshot come from the same resolution that
/// opened the reader. Retaining them with the bytes lets UTF-8 conversion
/// report the same diagnostic context after the read has completed.
pub(super) struct CompleteRead {
    /// Bytes delivered as the successful operation result.
    pub(super) bytes: Vec<u8>,
    /// Virtual absolute path used to open and diagnose the read.
    pub(super) path: PathBuf,
    /// PWD snapshot used to resolve a relative request, when applicable.
    pub(super) current_directory: Option<PathBuf>,
}
