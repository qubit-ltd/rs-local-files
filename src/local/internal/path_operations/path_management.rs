// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private path resolution, preparation, and cleanup operations.
// qubit-style: allow source-test-pair

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
use super::super::file_move::remove_directory_symlink;
use super::super::path_io_error::PathIoError;

/// Coverage-only access to private path-management operations.
#[cfg(coverage)]
pub fn coverage_absolute_path(path: &Path) -> Result<PathBuf> {
    absolute_path(path)
}

#[cfg(coverage)]
pub fn coverage_canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    canonicalize_existing_prefix(path)
}

#[cfg(coverage)]
pub fn coverage_ensure_dir_path(path: &Path) -> Result<()> {
    ensure_dir_path(path)
}

#[cfg(coverage)]
pub fn coverage_ensure_parent_path(path: &Path) -> Result<()> {
    ensure_parent_path(path)
}

#[cfg(coverage)]
pub fn coverage_ensure_parent_path_with_sync_dirs(
    path: &Path,
) -> Result<Vec<PathBuf>> {
    ensure_parent_path_with_sync_dirs(path)
}

#[cfg(coverage)]
pub fn coverage_add_path_context(
    error: Error,
    operation: &'static str,
    path: &Path,
) -> Error {
    add_path_context(error, operation, path)
}

#[cfg(coverage)]
pub fn coverage_clean_dir_path(path: &Path) -> Result<()> {
    clean_dir_path(path)
}

#[cfg(coverage)]
pub fn coverage_remove_any_path(path: &Path) -> Result<()> {
    remove_any_path(path)
}

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
#[cfg_attr(coverage, inline(never))]
#[cfg_attr(not(coverage), inline(always))]
pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path)
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
///
/// # Panics
/// Panics if an absolute path is reported as missing after traversal reaches
/// its filesystem root.
pub(crate) fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    if path.try_exists()? {
        return fs::canonicalize(path);
    }
    if path.as_os_str().is_empty() {
        return fs::canonicalize(path);
    }
    let mut missing = Vec::<OsString>::new();
    let mut current = absolute_path(path)?;
    while !current.try_exists()? {
        let name = current
            .file_name()
            .expect("a missing absolute path should have a file name");
        missing.push(name.to_os_string());
        let _ = current.pop();
    }
    let mut canonical = fs::canonicalize(current)?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

/// Ensures that the directory at `path` exists.
///
/// # Parameters
/// - `path`: Directory path to create.
///
/// # Errors
/// Returns an I/O error when the directory or one of its ancestors cannot be
/// created.
#[cfg_attr(coverage, inline(never))]
#[cfg_attr(not(coverage), inline(always))]
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
#[cfg_attr(coverage, inline(never))]
#[cfg_attr(not(coverage), inline(always))]
pub(crate) fn add_path_context(
    error: Error,
    operation: &'static str,
    path: &Path,
) -> Error {
    Error::new(error.kind(), PathIoError::new(operation, path, error))
}

/// Removes all children from a directory while keeping the directory itself.
///
/// # Parameters
/// - `path`: Directory path to clean.
///
/// # Errors
/// Returns an I/O error when `path` is not a directory, cannot be read, or a
/// child cannot be removed.
#[allow(dead_code)]
pub(crate) fn clean_dir_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
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
#[allow(dead_code)]
pub(crate) fn remove_any_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        #[cfg(windows)]
        if metadata.file_type().is_symlink_dir() {
            return remove_directory_symlink(path);
        }
        fs::remove_file(path)
    }
}
