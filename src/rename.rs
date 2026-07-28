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

use crate::local::{move_directory_without_replacing, move_file_without_replacing};

/// Renames or moves a local path using the platform operation.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn move_path(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)
}

/// Renames a local path only when the destination does not exist.
///
/// The operation uses the platform's atomic no-replace primitive when
/// available. Unsupported targets return [`std::io::ErrorKind::Unsupported`]
/// instead of emulating the guarantee with a racy existence check.
///
/// # Errors
/// Returns an I/O error when the source cannot be inspected, the destination
/// exists, the platform lacks a no-replace primitive, or the move fails.
pub fn move_path_without_replacing(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        move_directory_without_replacing(source, destination)
    } else {
        move_file_without_replacing(source, destination)
    }
}
