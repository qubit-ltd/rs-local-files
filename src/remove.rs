// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local path removal operations.

use std::fs;
use std::io::Result;
use std::path::Path;

use crate::local::remove_any_path;

/// Removes a file.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn file(path: &Path) -> Result<()> {
    fs::remove_file(path)
}

/// Removes an empty directory.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn empty_directory(path: &Path) -> Result<()> {
    fs::remove_dir(path)
}

/// Removes a directory tree.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn directory_tree(path: &Path) -> Result<()> {
    fs::remove_dir_all(path)
}

/// Removes a file, symbolic link, or directory tree according to its type.
///
/// # Errors
/// Returns an I/O error when the path cannot be inspected or removed.
#[inline(always)]
pub fn any(path: &Path) -> Result<()> {
    remove_any_path(path)
}
