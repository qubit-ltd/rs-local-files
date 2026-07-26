// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native local file write opening.

use std::fs::File;
use std::io;
use std::path::Path;

use super::OpenOptions;

/// Opens one local regular file for unbuffered writing.
///
/// # Errors
/// Returns an I/O error when parent creation, path inspection, opening, or
/// required truncation fails.
#[inline]
pub fn open(path: &Path, options: &OpenOptions) -> io::Result<File> {
    crate::local::open_native_writer_path(path, options)
}
