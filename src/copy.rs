// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive local directory copying.

use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};

use crate::local::copy_dir_all_with_paths;

pub use crate::local::{
    LocalCopyConflictPolicy as ConflictPolicy, LocalCopyDirError as Error,
    LocalCopyDirOptions as Options, LocalCopyDirStage as Stage, LocalCopyDirStats as Statistics,
    LocalCopyTypeConflictPolicy as TypeConflictPolicy,
};

/// Recursively copies a directory tree with explicit policies.
///
/// # Errors
/// Returns a structured error containing the failed stage and partial
/// statistics.
#[inline(always)]
pub fn directory(source: &Path, destination: &Path, options: Options) -> Result<Statistics, Error> {
    copy_dir_all_with_paths(source, destination, options)
}

/// Copies one file and replaces an existing destination.
///
/// # Returns
/// The number of content bytes copied.
///
/// # Errors
/// Returns the I/O error reported while reading the source, writing the
/// destination, or copying portable permissions.
#[inline(always)]
pub fn file(source: &Path, destination: &Path) -> io::Result<u64> {
    fs::copy(source, destination)
}

/// Copies one file only when the destination does not exist.
///
/// A destination created before a later copy or permission failure is removed
/// on a best-effort basis. The original operation error is retained.
///
/// # Returns
/// The number of content bytes copied.
///
/// # Errors
/// Returns [`std::io::ErrorKind::AlreadyExists`] for a destination conflict,
/// or the I/O error reported while reading, writing, or applying permissions.
pub fn file_without_replacing(source: &Path, destination: &Path) -> io::Result<u64> {
    let mut source_file = fs::File::open(source)?;
    let permissions = source_file.metadata()?.permissions();
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let result = io::copy(&mut source_file, &mut destination_file).and_then(|bytes| {
        destination_file.sync_all()?;
        fs::set_permissions(destination, permissions)?;
        Ok(bytes)
    });
    if result.is_err() {
        drop(destination_file);
        let _ = fs::remove_file(destination);
    }
    result
}
