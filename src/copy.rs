// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive local directory copying.

use std::path::Path;

use crate::local::copy_dir_all_with_paths;

pub use crate::local::{
    LocalCopyConflictPolicy as ConflictPolicy,
    LocalCopyDirError as Error,
    LocalCopyDirOptions as Options,
    LocalCopyDirStage as Stage,
    LocalCopyDirStats as Statistics,
    LocalCopyTypeConflictPolicy as TypeConflictPolicy,
};

/// Recursively copies a directory tree with explicit policies.
///
/// # Errors
/// Returns a structured error containing the failed stage and partial
/// statistics.
#[inline(always)]
pub fn directory(
    source: &Path,
    destination: &Path,
    options: Options,
) -> Result<Statistics, Error> {
    copy_dir_all_with_paths(source, destination, options)
}
