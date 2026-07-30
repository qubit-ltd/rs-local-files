// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native local file read opening.

use std::fs::File;
use std::io;
use std::path::Path;

use super::OpenOptions;

/// Opens one local regular file for unbuffered reading with default options.
///
/// # Errors
/// Returns an I/O error when the path cannot be inspected or opened, or when
/// the opened object is not a regular file.
#[inline(always)]
pub fn open(path: &Path) -> io::Result<File> {
    open_with(path, &OpenOptions::default())
}

/// Opens one local regular file for unbuffered reading with explicit options.
///
/// # Parameters
/// - `path`: File path to inspect and open.
/// - `options`: Read-open retry policy.
///
/// # Returns
/// The validated standard-library file handle.
///
/// # Errors
/// Returns an I/O error when the path cannot be inspected or opened, or when
/// the opened object is not a regular file.
#[inline(always)]
pub fn open_with(path: &Path, options: &OpenOptions) -> io::Result<File> {
    crate::local::open_native_reader_path(path, options)
}
