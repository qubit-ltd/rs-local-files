// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local directory operations.

use std::fs::{
    self,
    ReadDir,
};
use std::io::Result;
use std::path::Path;

use crate::local::{
    clean_dir_path,
    dir_size_path,
    ensure_dir_path,
    ensure_parent_path,
};

/// Lists the direct entries of a directory.
///
/// # Errors
/// Returns the I/O error reported by the filesystem.
#[inline(always)]
pub fn read(path: &Path) -> Result<ReadDir> {
    fs::read_dir(path)
}

/// Creates a directory and any missing ancestors.
///
/// # Errors
/// Returns an I/O error when the directory cannot be created.
#[inline(always)]
pub fn create_all(path: &Path) -> Result<()> {
    ensure_dir_path(path)
}

/// Creates the parent directory of a path and any missing ancestors.
///
/// # Errors
/// Returns an I/O error when the parent directory cannot be created.
#[inline(always)]
pub fn create_parent(path: &Path) -> Result<()> {
    ensure_parent_path(path)
}

/// Computes the total byte length of regular files in a directory tree.
///
/// # Errors
/// Returns an I/O error when the tree cannot be traversed or inspected.
#[inline(always)]
pub fn size(path: &Path) -> Result<u64> {
    dir_size_path(path)
}

/// Removes every direct child of a directory while retaining the directory.
///
/// # Errors
/// Returns an I/O error when the directory cannot be read or a child cannot be
/// removed.
#[inline(always)]
pub fn clear(path: &Path) -> Result<()> {
    clean_dir_path(path)
}
