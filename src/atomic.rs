// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Durable atomic local file replacement.

use std::io;
use std::path::Path;

pub use crate::local::{
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

/// Atomically writes bytes and creates missing parent directories.
///
/// # Parameters
/// - `path`: Destination path to replace.
/// - `bytes`: Complete destination contents.
///
/// # Errors
/// Returns a structured error when staging, writing, synchronization, metadata
/// preservation, installation, or parent synchronization fails.
#[inline(always)]
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    begin(path)?.write_bytes(bytes)
}

/// Atomically writes a file with caller-provided streaming logic.
///
/// The callback receives the guarded staging writer. A callback error aborts
/// publication and is retained as the source of the returned structured error.
///
/// # Type Parameters
/// - `F`: One-shot callback that writes staging contents.
///
/// # Parameters
/// - `path`: Destination path to replace.
/// - `write`: Callback that writes complete destination contents.
///
/// # Errors
/// Returns a structured error when staging, the callback, synchronization,
/// metadata preservation, installation, cleanup, or parent synchronization
/// fails.
#[inline(always)]
pub fn write_with<F>(path: &Path, write: F) -> Result<(), Error>
where
    F: FnOnce(&mut Writer) -> io::Result<()>,
{
    begin(path)?.write_with(write)
}
