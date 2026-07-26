// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local path rename operations.

use std::fs;
use std::io::Result;
use std::path::Path;

/// Renames or moves a local path using the platform operation.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn move_path(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)
}
