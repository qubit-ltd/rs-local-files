// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local filesystem metadata operations.

use std::fs::{
    self,
    Metadata,
};
use std::io::Result;
use std::path::Path;

/// Tests whether a local path exists without hiding inspection errors.
///
/// # Errors
/// Returns an I/O error when the filesystem cannot determine whether `path`
/// exists.
#[inline(always)]
pub fn exists(path: &Path) -> Result<bool> {
    path.try_exists()
}

/// Reads metadata while following symbolic links.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn read(path: &Path) -> Result<Metadata> {
    fs::metadata(path)
}

/// Reads metadata without following the final symbolic link.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn read_link(path: &Path) -> Result<Metadata> {
    fs::symlink_metadata(path)
}
