// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private path resolution, preparation, and cleanup operations.

use std::ffi::OsString;
use std::fs;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
#[cfg(windows)]
use std::os::windows::fs::FileTypeExt;
use std::path::Path;
use std::path::PathBuf;

#[cfg(windows)]
use super::super::file_move::remove_directory_symlink;
use super::super::path_io_error::PathIoError;

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
        let Some(name) = current.file_name() else {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("path has no existing prefix: {}", path.display()),
            ));
        };
        missing.push(name.to_os_string());
        if !current.pop() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("path has no existing prefix: {}", path.display()),
            ));
        }
    }
    let mut canonical = fs::canonicalize(current)?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

/// Canonicalizes an existing prefix while sharing the copy operation budget.
///
/// # Parameters
/// - `path`: Path whose existing prefix is canonicalized.
/// - `budget`: Copy budget checked around native path operations.
///
/// # Errors
/// Returns the native path error or a budget timeout.
pub(crate) fn canonicalize_existing_prefix_with_budget(
    path: &Path,
    budget: &crate::local::CopyBudget,
) -> Result<PathBuf> {
    budget.check_deadline()?;
    if path.try_exists()? {
        budget.check_deadline()?;
        let canonical = fs::canonicalize(path)?;
        budget.check_deadline()?;
        return Ok(canonical);
    }
    if path.as_os_str().is_empty() {
        budget.check_deadline()?;
        let canonical = fs::canonicalize(path)?;
        budget.check_deadline()?;
        return Ok(canonical);
    }
    let mut missing = Vec::<OsString>::new();
    let mut current = absolute_path(path)?;
    budget.check_deadline()?;
    while !current.try_exists()? {
        budget.check_deadline()?;
        let Some(name) = current.file_name() else {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("path has no existing prefix: {}", path.display()),
            ));
        };
        missing.push(name.to_os_string());
        if !current.pop() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("path has no existing prefix: {}", path.display()),
            ));
        }
    }
    budget.check_deadline()?;
    let mut canonical = fs::canonicalize(current)?;
    budget.check_deadline()?;
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
pub(crate) fn ensure_parent_path_with_sync_dirs(path: &Path) -> Result<Vec<PathBuf>> {
    let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) else {
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
                    format!("parent path component is not a directory: {}", current.display()),
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

/// Creates missing parent components with a deadline check between native
/// inspections and each individual directory creation. The boolean on error
/// reports whether this call already created a parent.
///
/// # Parameters
/// - `path`: Destination path whose parent components may need creation.
/// - `budget`: Copy budget checked between inspections and creations.
///
/// # Returns
/// Returns created parent paths, or the error and whether any parent was
/// created.
pub(crate) fn ensure_parent_path_with_budget(
    path: &Path,
    budget: &crate::local::CopyBudget,
) -> std::result::Result<Vec<PathBuf>, (Error, bool)> {
    let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) else {
        return Ok(Vec::new());
    };
    let mut missing = Vec::new();
    let mut current = parent;
    loop {
        budget.check_deadline().map_err(|error| (error, false))?;
        match fs::metadata(current) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err((
                    Error::new(ErrorKind::AlreadyExists, "copy parent component is not a directory"),
                    false,
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => missing.push(current.to_path_buf()),
            Err(error) => return Err((error, false)),
        }
        budget.check_deadline().map_err(|error| (error, false))?;
        let Some(next) = current.parent().filter(|next| !next.as_os_str().is_empty()) else {
            break;
        };
        current = next;
    }
    #[cfg(test)]
    crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
        crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::HostCopyParentPreflightComplete,
    );
    budget.check_deadline().map_err(|error| (error, false))?;
    missing.reverse();
    let mut created = false;
    for directory in &missing {
        budget.check_deadline().map_err(|error| (error, created))?;
        match fs::create_dir(directory) {
            Ok(()) => created = true,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                budget.check_deadline().map_err(|error| (error, created))?;
                if !fs::metadata(directory).is_ok_and(|metadata| metadata.is_dir()) {
                    return Err((error, created));
                }
            }
            Err(error) => return Err((error, created)),
        }
        #[cfg(test)]
        if created {
            crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
                crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::HostCopyParentCreated,
            );
        }
        budget.check_deadline().map_err(|error| (error, created))?;
    }
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
#[must_use]
pub(crate) fn add_path_context(error: Error, operation: &'static str, path: &Path) -> Error {
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
