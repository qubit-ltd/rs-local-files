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

#[cfg(windows)]
use super::file_move::remove_directory_symlink;
use super::path_io_error::PathIoError;

/// Resolves `path` to a lexical absolute path at the current point in time.
///
/// # Parameters
/// - `path`: Path to bind to the current working directory when relative.
///
/// # Returns
/// An absolute path suitable for operations that may occur after the process
/// current directory changes.
///
/// # Errors
/// Returns an I/O error when a relative path is supplied and the current
/// working directory cannot be read.
#[inline]
pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path)
}

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

/// Ensures the parent directory of `path` and records directories whose parent
/// entries require synchronization.
///
/// Directories observed as missing are returned even when another caller races
/// to create them before [`fs::create_dir_all`] completes. Synchronizing an
/// extra parent directory is safe and preserves the durability guarantee.
///
/// # Parameters
/// - `path`: File path whose parent directory should be created.
///
/// # Returns
/// Directories observed as missing, ordered from shallowest to deepest.
///
/// # Errors
/// Returns an I/O error when a parent component cannot be inspected or
/// created, or an existing component is not a directory.
pub(crate) fn ensure_parent_path_with_sync_dirs(
    path: &Path,
) -> Result<Vec<PathBuf>> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(Vec::new());
    };
    let mut missing = Vec::new();
    let mut current = parent;
    loop {
        match fs::metadata(current) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!(
                        "parent path component is not a directory: {}",
                        current.display()
                    ),
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
            }
            Err(error) => return Err(error),
        }
        match current.parent() {
            Some(next) if !next.as_os_str().is_empty() => current = next,
            _ => break,
        }
    }

    ensure_dir_path(parent)?;
    missing.reverse();
    Ok(missing)
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
pub(crate) fn add_path_context(
    error: Error,
    operation: &'static str,
    path: &Path,
) -> Error {
    Error::new(error.kind(), PathIoError::new(operation, path, error))
}

// A portable test fixture cannot reliably provision more than `u64::MAX` bytes
// of aggregate file length. Keep construction inline so that limitation does
// not create an otherwise unreachable helper function in coverage data.
macro_rules! dir_size_overflow_error {
    ($path:expr) => {
        Error::new(
            ErrorKind::InvalidData,
            format!("directory size exceeds u64 at {}", $path.display()),
        )
    };
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
/// Returns an I/O error when a directory entry cannot be read, or
/// [`ErrorKind::InvalidData`] when the aggregate exceeds [`u64::MAX`].
fn dir_size_recursive(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        let contribution = if metadata.is_dir() {
            dir_size_recursive(&entry_path)?
        } else if metadata.is_file() {
            metadata.len()
        } else {
            0
        };
        total = match total.checked_add(contribution) {
            Some(total) => total,
            None => return Err(dir_size_overflow_error!(&entry_path)),
        };
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
            return remove_directory_symlink(path);
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
    if path.try_exists()? {
        return fs::canonicalize(path);
    }
    let mut missing = Vec::<OsString>::new();
    let mut current = path.to_path_buf();
    while !current.try_exists()? {
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
