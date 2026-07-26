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

/// Opens one local regular file for unbuffered reading.
///
/// # Errors
/// Returns an I/O error when the path cannot be inspected or opened, or when
/// the opened object is not a regular file.
#[inline]
pub fn open(path: &Path, options: &OpenOptions) -> io::Result<File> {
    crate::local::open_native_reader_path(path, options)
}
