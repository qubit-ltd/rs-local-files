// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private durable atomic-write pipeline.

use std::fs::{
    self,
    File,
};
use std::io::{
    ErrorKind,
    Result,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};

use crate::{
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
};

use super::file_move::{
    parent_dir_for,
    replace_file,
    sync_parent_dir,
};
use super::path_operations::{
    add_path_context,
    ensure_parent_path_with_sync_dirs,
};
use super::temp_entry::create_temp_file_in_dir;
use super::{
    DEFAULT_TEMP_FILE_RETRIES,
    StagedFile,
};

/// Default suffix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_SUFFIX: &str = ".tmp";

/// Prefix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_PREFIX: &str = ".atomic-write-";

/// Atomically writes `bytes` to `path`.
///
/// # Parameters
/// - `path`: Destination path.
/// - `bytes`: Bytes to write.
///
/// # Errors
/// Returns a structured atomic-write error retaining the failed stage,
/// temporary path, commit state, and native I/O source error.
pub(crate) fn atomic_write_bytes_path(
    path: &Path,
    bytes: &[u8],
) -> std::result::Result<(), LocalAtomicWriteError> {
    atomic_write_with_path(path, &mut |file| file.write_all(bytes))
}

/// Adds atomic-write context to a native I/O result.
fn with_atomic_context<T>(
    result: Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    temporary_path: Option<PathBuf>,
    committed: bool,
) -> std::result::Result<T, LocalAtomicWriteError> {
    result.map_err(|source| {
        LocalAtomicWriteError::new(
            stage,
            path.to_path_buf(),
            temporary_path,
            committed,
            source,
        )
    })
}

/// Creates an atomic-write error and explicitly cleans up its staging file.
///
/// # Parameters
/// - `stage`: Stage at which the primary operation failed.
/// - `path`: Requested destination path.
/// - `source`: Primary I/O error.
/// - `staged_file`: Uncommitted staging file to close and remove.
///
/// # Returns
/// Structured primary error with any secondary cleanup error attached.
fn atomic_error_with_staging(
    stage: LocalAtomicWriteStage,
    path: &Path,
    source: std::io::Error,
    staged_file: &mut StagedFile,
) -> LocalAtomicWriteError {
    let temporary_path = staged_file.path().to_path_buf();
    let cleanup_error = staged_file.cleanup().err();
    LocalAtomicWriteError::new(
        stage,
        path.to_path_buf(),
        Some(temporary_path),
        false,
        source,
    )
    .with_cleanup_error(cleanup_error)
}

/// Atomically writes a file at `path` using `write`.
///
/// # Parameters
/// - `path`: Destination path.
/// - `write`: Function that writes the desired contents into the temporary
///   file.
///
/// # Errors
/// Returns a structured atomic-write error retaining the failed stage,
/// temporary path, commit state, and native I/O source error.
///
/// # Panics
/// Propagates panics from `write`. While unwinding, [`StagedFile`] performs
/// best-effort cleanup of the temporary file.
pub(crate) fn atomic_write_with_path(
    path: &Path,
    write: &mut dyn FnMut(&mut File) -> Result<()>,
) -> std::result::Result<(), LocalAtomicWriteError> {
    let parent_dirs_to_sync = with_atomic_context(
        ensure_parent_path_with_sync_dirs(path),
        LocalAtomicWriteStage::PrepareParent,
        path,
        None,
        false,
    )?;
    let existing_permissions = with_atomic_context(
        existing_file_permissions(path),
        LocalAtomicWriteStage::InspectDestination,
        path,
        None,
        false,
    )?;
    let parent = parent_dir_for(path);
    let (temp_path, file) = with_atomic_context(
        create_temp_file_in_dir(
            parent,
            Some(ATOMIC_WRITE_TEMP_PREFIX),
            Some(ATOMIC_WRITE_TEMP_SUFFIX),
            DEFAULT_TEMP_FILE_RETRIES,
        ),
        LocalAtomicWriteStage::CreateTemporaryFile,
        path,
        None,
        false,
    )?;
    let mut staged_file = StagedFile::new(temp_path, file);

    if let Err(source) = write(staged_file.file_mut()) {
        return Err(atomic_error_with_staging(
            LocalAtomicWriteStage::WriteTemporaryFile,
            path,
            source,
            &mut staged_file,
        ));
    }
    if let Err(source) = apply_existing_permissions(
        staged_file.file(),
        existing_permissions.as_ref(),
        staged_file.path(),
    ) {
        return Err(atomic_error_with_staging(
            LocalAtomicWriteStage::PreservePermissions,
            path,
            source,
            &mut staged_file,
        ));
    }
    if let Err(source) = staged_file.file().sync_all() {
        return Err(atomic_error_with_staging(
            LocalAtomicWriteStage::SyncTemporaryFile,
            path,
            source,
            &mut staged_file,
        ));
    }

    staged_file.close();
    if let Err(source) = replace_file(staged_file.path(), path) {
        return Err(atomic_error_with_staging(
            LocalAtomicWriteStage::ReplaceDestination,
            path,
            source,
            &mut staged_file,
        ));
    }
    let temp_path = staged_file.path().to_path_buf();
    staged_file.disarm();
    with_atomic_context(
        sync_atomic_parent_chain(path, &parent_dirs_to_sync),
        LocalAtomicWriteStage::SyncParentDirectory,
        path,
        Some(temp_path),
        true,
    )
}

/// Synchronizes the committed destination and every newly created parent
/// directory entry.
///
/// # Parameters
/// - `path`: Committed destination path.
/// - `parent_dirs_to_sync`: Parent directories observed as missing before
///   staging, ordered from shallowest to deepest.
///
/// # Errors
/// Returns the I/O error reported while opening or synchronizing a directory.
fn sync_atomic_parent_chain(
    path: &Path,
    parent_dirs_to_sync: &[PathBuf],
) -> Result<()> {
    sync_parent_dir(path)?;
    for directory in parent_dirs_to_sync.iter().rev() {
        sync_parent_dir(directory)?;
    }
    Ok(())
}

/// Returns existing destination permissions to preserve during atomic writes.
///
/// # Parameters
/// - `path`: Destination file path.
///
/// # Returns
/// Existing file permissions when `path` points to a regular file.
///
/// # Errors
/// Returns an I/O error when destination metadata exists but cannot be read.
fn existing_file_permissions(path: &Path) -> Result<Option<fs::Permissions>> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file() && !metadata.file_type().is_symlink() =>
        {
            Ok(Some(metadata.permissions()))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(add_path_context(error, "read destination metadata", path))
        }
    }
}

/// Applies preserved destination permissions to the temporary file.
///
/// # Parameters
/// - `file`: Temporary file handle.
/// - `permissions`: Optional permissions to apply.
/// - `temp_path`: Temporary file path used for error context.
///
/// # Errors
/// Returns an I/O error when permissions cannot be applied.
fn apply_existing_permissions(
    file: &File,
    permissions: Option<&fs::Permissions>,
    temp_path: &Path,
) -> Result<()> {
    if let Some(permissions) = permissions
        && let Err(error) = file.set_permissions(permissions.clone())
    {
        return Err(add_path_context(
            error,
            "set temporary file permissions",
            temp_path,
        ));
    }
    Ok(())
}
