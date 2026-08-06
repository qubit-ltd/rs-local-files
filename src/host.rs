// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Convenience functions for the process-visible Host filesystem.
//!
//! These functions use [`crate::LocalSymlinkPolicy::FollowAcrossScope`] for
//! intermediate path components. Final components retain native operation
//! semantics: metadata, delete, rename, copy targets, and temporary-resource
//! persistence operate on the link entry, while readers and writers that
//! address existing content follow it.
// qubit-style: allow source-test-pair

use std::path::Path;

use crate::{
    LocalCopyOptions,
    LocalCopyResult,
    LocalCreateDirectoryOptions,
    LocalCreateDirectoryOutcome,
    LocalDeleteOptions,
    LocalDeleteOutcome,
    LocalDirectoryWalker,
    LocalFileMetadata,
    LocalFileReader,
    LocalFileSystem,
    LocalFileWriter,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalRenameResult,
    LocalResult,
    LocalTempDirectory,
    LocalTempDirectoryOptions,
    LocalTempFile,
    LocalTempFileOptions,
    LocalWriteOptions,
};

/// Reads Host metadata without following the final symbolic link.
pub fn metadata(path: &Path) -> LocalResult<LocalFileMetadata> {
    LocalFileSystem::host().metadata(path)
}

/// Opens a Host regular-file reader.
pub fn open_reader(
    path: &Path,
    options: &LocalReadOptions,
) -> LocalResult<LocalFileReader> {
    LocalFileSystem::host().open_reader(path, options)
}

/// Reads at most max_bytes from a Host regular file.
pub fn read_prefix(
    path: &Path,
    options: &LocalReadOptions,
    max_bytes: usize,
) -> LocalResult<Vec<u8>> {
    LocalFileSystem::host().read_prefix(path, options, max_bytes)
}

/// Opens a Host writer publication session.
pub fn open_writer(
    path: &Path,
    options: &LocalWriteOptions,
) -> LocalResult<LocalFileWriter> {
    LocalFileSystem::host().open_writer(path, options)
}

/// Opens a lazy Host directory walker.
pub fn list(
    path: &Path,
    options: &LocalListOptions,
) -> LocalResult<LocalDirectoryWalker> {
    LocalFileSystem::host().list(path, options)
}

/// Copies one Host regular file or directory tree.
#[allow(clippy::result_large_err)]
pub fn copy(
    source: &Path,
    destination: &Path,
    options: &LocalCopyOptions,
) -> LocalCopyResult {
    LocalFileSystem::host().copy(source, destination, options)
}

/// Creates a Host directory.
pub fn create_directory(
    path: &Path,
    options: &LocalCreateDirectoryOptions,
) -> LocalResult<LocalCreateDirectoryOutcome> {
    LocalFileSystem::host().create_directory(path, options)
}

/// Deletes a Host non-directory entry.
pub fn delete_file(
    path: &Path,
    options: &LocalDeleteOptions,
) -> LocalResult<LocalDeleteOutcome> {
    LocalFileSystem::host().delete_file(path, options)
}

/// Deletes a Host directory according to the recursion policy.
pub fn delete_directory(
    path: &Path,
    options: &LocalDeleteOptions,
) -> LocalResult<LocalDeleteOutcome> {
    LocalFileSystem::host().delete_directory(path, options)
}

/// Renames one Host entry.
#[allow(clippy::result_large_err)]
pub fn rename(
    source: &Path,
    destination: &Path,
    options: &LocalRenameOptions,
) -> LocalRenameResult {
    LocalFileSystem::host().rename(source, destination, options)
}

/// Creates a cleanup-owned Host temporary file.
pub fn create_temp_file(
    options: &LocalTempFileOptions,
) -> LocalResult<LocalTempFile> {
    LocalFileSystem::host().create_temp_file(options)
}

/// Creates a cleanup-owned Host temporary directory.
pub fn create_temp_directory(
    options: &LocalTempDirectoryOptions,
) -> LocalResult<LocalTempDirectory> {
    LocalFileSystem::host().create_temp_directory(options)
}
