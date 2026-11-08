// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalFileError;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalWriteMode;
use super::LocalWriteOptions;
use super::Path;
use super::io;

/// Opens the shared same-directory staged writer implementation.
///
/// # Parameters
/// - `path`: Bound destination path.
/// - `options`: Unified writer options.
///
/// # Returns
/// Open staged writer.
///
/// # Errors
/// Returns `LocalFileError` when staging cannot be prepared.
pub(crate) fn open_staged_writer(
    path: &Path,
    options: &LocalWriteOptions,
) -> LocalResult<crate::local::LocalAtomicWriter> {
    let mut native_options = crate::local::LocalAtomicWriteOptions::new()
        .with_durability(options.durability())
        .with_file_sync(options.file_sync())
        .with_metadata_policy(options.metadata_policy());
    if options.mode() == LocalWriteMode::CreateNew {
        native_options = native_options.with_create_new();
    }
    if options.creates_parent() {
        native_options = native_options.with_create_parent();
    }
    if let Some(timeout) = options.open_retry_timeout() {
        native_options = native_options.with_open_retry_timeout(timeout);
    }
    crate::local::LocalAtomicWriter::new(path, native_options).map_err(|error| {
        let kind = error.kind();
        LocalFileError::from_io(
            LocalFileOperation::OpenWriter,
            Some(path.to_path_buf()),
            None,
            io::Error::new(kind, error),
        )
    })
}
