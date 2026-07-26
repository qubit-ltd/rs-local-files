// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Durable atomic local file replacement.

use std::path::Path;

pub use crate::{
    LocalAtomicCommitError as CommitError,
    LocalAtomicDestinationState as DestinationState,
    LocalAtomicWriteError as Error,
    LocalAtomicWriteOptions as Options,
    LocalAtomicWriteStage as Stage,
    LocalAtomicWriter as Writer,
};

/// Begins an atomic replacement and creates missing parent directories.
///
/// # Errors
/// Returns a structured error when preparation or staging-file creation fails.
#[inline(always)]
pub fn begin(path: &Path) -> Result<Writer, Error> {
    Writer::new(path, Options::new().with_parent())
}

/// Begins an atomic replacement with explicit options.
///
/// # Errors
/// Returns a structured error when preparation or staging-file creation fails.
#[inline(always)]
pub fn begin_with(path: &Path, options: Options) -> Result<Writer, Error> {
    Writer::new(path, options)
}
