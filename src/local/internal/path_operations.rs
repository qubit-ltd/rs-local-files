// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private path and directory operations.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::{
    Path,
    PathBuf,
};

#[cfg(windows)]
use std::os::windows::fs::FileTypeExt;

use super::path_io_error::PathIoError;

/// Ensures that the directory at `path` exists.
///
/// # Parameters
/// - `path`: Directory path to create.
///
/// # Errors
/// Returns an I/O error when the directory or one of its ancestors cannot be
/// created.
pub(crate) fn ensure_dir_path(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
}

/// Ensures that the parent directory of `path` exists.
///
/// # Parameters
/// - `path`: File path whose parent directory should be created.
///
/// # Errors
/// Returns an I/O error when the parent directory or one of its ancestors
/// cannot be created.
pub(crate) fn ensure_parent_path(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        ensure_dir_path(parent)?;
    }
    Ok(())
}

/// Adds path context to an I/O error while preserving its kind and source.
///
/// # Parameters
/// - `error`: Original I/O error.
/// - `operation`: Operation that failed.
/// - `path`: Path involved in the operation.
///
/// # Returns
/// A new I/O error with the same [`ErrorKind`] and a more descriptive message.
pub(super) fn add_path_context(
    error: Error,
    operation: &'static str,
    path: &Path,
) -> Error {
    Error::new(error.kind(), PathIoError::new(operation, path, error))
}

/// Computes the total size of regular files below a directory path.
///
/// # Parameters
/// - `path`: Directory path to measure.
///
/// # Returns
/// The total byte length of regular files under `path`.
///
/// # Errors
/// Returns an I/O error when `path` is not a directory or cannot be read.
pub(crate) fn dir_size_path(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path is not a directory: {}", path.display()),
        ));
    }
    dir_size_recursive(path)
}

/// Recursively computes regular-file sizes below a directory.
///
/// # Parameters
/// - `path`: Directory path to measure.
///
/// # Returns
/// The total byte length of regular files under `path`.
///
/// # Errors
/// Returns an I/O error when a directory entry cannot be read.
fn dir_size_recursive(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            total += dir_size_recursive(&entry.path())?;
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

/// Removes all children from a directory while keeping the directory itself.
///
/// # Parameters
/// - `path`: Directory path to clean.
///
/// # Errors
/// Returns an I/O error when `path` is not a directory, cannot be read, or a
/// child cannot be removed.
pub(crate) fn clean_dir_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path is not a directory: {}", path.display()),
        ));
    }
    for entry in fs::read_dir(path)? {
        remove_any_path(&entry?.path())?;
    }
    Ok(())
}

/// Removes a path regardless of whether it is a file, directory, or symlink.
///
/// # Parameters
/// - `path`: Path to remove.
///
/// # Errors
/// Returns an I/O error when `path` cannot be inspected or removed.
pub(crate) fn remove_any_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if metadata.is_dir() && !file_type.is_symlink() {
        fs::remove_dir_all(path)
    } else {
        #[cfg(windows)]
        if file_type.is_symlink_dir() {
            return fs::remove_dir(path);
        }
        fs::remove_file(path)
    }
}

/// Canonicalizes the existing prefix of a path while preserving missing tail
/// components.
///
/// # Parameters
/// - `path`: Path that may not exist yet.
///
/// # Returns
/// A canonicalized path for the existing prefix with missing components
/// appended.
///
/// # Errors
/// Returns an I/O error when the existing prefix cannot be canonicalized.
pub(super) fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path);
    }
    let mut missing = Vec::<OsString>::new();
    let mut current = path.to_path_buf();
    while !current.exists() {
        if let Some(name) = current.file_name() {
            missing.push(name.to_os_string());
        } else {
            break;
        }
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                current = parent.to_path_buf()
            }
            _ => {
                current = env::current_dir()?;
                break;
            }
        }
    }
    let mut canonical = fs::canonicalize(current)?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}
