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

/// Adds path context to an I/O error while preserving its kind and source.
///
/// # Parameters
/// - `error`: Original I/O error.
/// - `operation`: Operation that failed.
/// - `path`: Path involved in the operation.
///
/// # Returns
/// A new I/O error with the same [`ErrorKind`] and a more descriptive message.
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

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;
    use std::fs;
    use std::io::Error;
    use std::io::ErrorKind;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use tempfile::tempdir;

    use super::absolute_path;
    use super::add_path_context;
    use super::canonicalize_existing_prefix;
    use super::clean_dir_path;
    use super::ensure_parent_path;
    use super::ensure_parent_path_with_sync_dirs;
    use super::remove_any_path;

    /// Verifies lexical absolute paths are preserved and existing prefixes are
    /// canonicalized without requiring the missing tail to exist.
    #[test]
    fn test_canonicalize_existing_prefix_preserves_missing_tail() {
        let directory = tempdir().expect("temporary directory should be created");
        let existing = directory.path().join("existing");
        fs::create_dir(&existing).expect("existing prefix should be created");
        let missing = existing.join("nested").join("payload");

        assert_eq!(
            fs::canonicalize(&existing)
                .expect("existing prefix should canonicalize")
                .join("nested")
                .join("payload"),
            canonicalize_existing_prefix(&missing).expect("missing tail should be preserved"),
        );
        assert_eq!(
            fs::canonicalize(&existing).expect("existing path should canonicalize"),
            canonicalize_existing_prefix(&existing).expect("existing path should canonicalize"),
        );
        assert_eq!(
            missing,
            absolute_path(&missing).expect("absolute input should be preserved")
        );
    }

    /// Verifies parent preparation reports every newly created directory in
    /// shallow-to-deep order and performs no work for a leaf-only path.
    #[test]
    fn test_ensure_parent_path_reports_created_sync_directories() {
        let directory = tempdir().expect("temporary directory should be created");
        let first = directory.path().join("first");
        let second = first.join("second");
        let file = second.join("payload");

        assert_eq!(
            vec![first.clone(), second.clone()],
            ensure_parent_path_with_sync_dirs(&file).expect("missing parents should be created"),
        );
        assert!(second.is_dir());
        assert!(
            ensure_parent_path_with_sync_dirs(&file)
                .expect("existing parents should be accepted")
                .is_empty(),
        );
        ensure_parent_path(Path::new("payload")).expect("a leaf-only path has no parent to create");
    }

    /// Verifies a non-directory parent is rejected before descendant creation.
    #[test]
    fn test_ensure_parent_path_rejects_non_directory_component() {
        let directory = tempdir().expect("temporary directory should be created");
        let parent = directory.path().join("not-a-directory");
        fs::write(&parent, b"payload").expect("conflicting file should be created");

        let error =
            ensure_parent_path_with_sync_dirs(&parent.join("child")).expect_err("a file parent must be rejected");
        assert_eq!(ErrorKind::AlreadyExists, error.kind());
        assert!(error.to_string().contains("not a directory"));
    }

    /// Verifies added path context retains the native kind, source, operation,
    /// and path for diagnostics.
    #[test]
    fn test_add_path_context_retains_native_error() {
        let native = Error::new(ErrorKind::PermissionDenied, "native denial");
        let error = add_path_context(native, "open", Path::new("private/payload"));

        assert_eq!(ErrorKind::PermissionDenied, error.kind());
        assert!(error.to_string().contains("failed to open 'private/payload'"));
        assert!(error.source().is_some());
    }

    /// Verifies recursive cleanup keeps its root while removing files and
    /// nested directories, and rejects a non-directory root.
    #[test]
    fn test_clean_dir_path_removes_children_but_keeps_root() {
        let directory = tempdir().expect("temporary directory should be created");
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).expect("nested directory should be created");
        fs::write(nested.join("payload"), b"payload").expect("nested file should be created");
        fs::write(directory.path().join("sibling"), b"sibling").expect("sibling file should be created");

        clean_dir_path(directory.path()).expect("directory children should be removed");
        assert!(directory.path().is_dir());
        assert_eq!(
            0,
            fs::read_dir(directory.path())
                .expect("cleaned directory should remain readable")
                .count(),
        );

        let file = directory.path().join("file");
        fs::write(&file, b"payload").expect("file fixture should be created");
        assert_eq!(
            ErrorKind::InvalidInput,
            clean_dir_path(&file)
                .expect_err("a file cannot be cleaned as a directory")
                .kind(),
        );
    }

    /// Verifies generic removal deletes files and directory trees.
    #[test]
    fn test_remove_any_path_handles_files_and_directories() {
        let directory = tempdir().expect("temporary directory should be created");
        let file = directory.path().join("file");
        fs::write(&file, b"payload").expect("file fixture should be created");
        remove_any_path(&file).expect("file should be removed");
        assert!(!file.exists());

        let tree = directory.path().join("tree");
        fs::create_dir(&tree).expect("directory fixture should be created");
        fs::write(tree.join("payload"), b"payload").expect("tree file should be created");
        remove_any_path(&tree).expect("directory tree should be removed");
        assert!(!tree.exists());
    }

    /// Verifies removing a symbolic link never removes its referent.
    #[cfg(unix)]
    #[test]
    fn test_remove_any_path_unlinks_symbolic_link_only() {
        let directory = tempdir().expect("temporary directory should be created");
        let referent = directory.path().join("referent");
        let link = directory.path().join("link");
        fs::write(&referent, b"payload").expect("referent should be created");
        symlink(&referent, &link).expect("symbolic link should be created");

        remove_any_path(&link).expect("symbolic link should be removed");
        assert!(!link.exists());
        assert!(referent.exists());
    }
}
