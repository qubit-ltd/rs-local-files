// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host delete operations.
// qubit-style: allow source-test-pair

use super::HostLocalFileSystem;
use super::LocalDeleteOptions;
use super::LocalDeleteOutcome;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::delete_work::DeleteWork;
use super::fs;
use super::io;
use super::resolve_host_path;
use super::test_io_fault;

impl HostLocalFileSystem {
    /// Deletes a Host file or final symbolic-link entry using an explicit
    /// symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Native file or symbolic-link path.
    /// - `options`: Missing-entry policy.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether an entry was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the entry is a directory or removal fails.
    pub fn delete_file_with_policy(
        path: &Path,
        options: &LocalDeleteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let bound = resolve_host_path(path, symlink_policy, false)?;
        let Some(metadata) = metadata_for_delete(&bound, options, LocalFileOperation::DeleteFile)? else {
            return Ok(LocalDeleteOutcome::new(false));
        };
        if metadata.file_type().is_dir() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::DeleteFile).with_path(bound),
            );
        }
        match test_io_fault("local-fs-delete-file-remove").map_or_else(|| fs::remove_file(&bound), Err) {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(source) if options.missing_ok() && source.kind() == io::ErrorKind::NotFound => {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(source) => Err(LocalFileError::from_io(
                LocalFileOperation::DeleteFile,
                Some(bound),
                None,
                source,
            )),
        }
    }

    /// Deletes a Host directory without following a final symbolic link.
    ///
    /// # Parameters
    ///
    /// - `path`: Native directory path.
    /// - `options`: Recursion and missing-entry policy.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether a directory was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the entry is not a directory or removal
    /// fails.
    pub fn delete_directory_with_policy(
        path: &Path,
        options: &LocalDeleteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let bound = resolve_host_path(path, symlink_policy, false)?;
        let Some(metadata) = metadata_for_delete(&bound, options, LocalFileOperation::DeleteDirectory)? else {
            return Ok(LocalDeleteOutcome::new(false));
        };
        if !metadata.file_type().is_dir() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::DeleteDirectory)
                    .with_path(bound),
            );
        }
        if options.recursive() {
            return remove_host_directory_tree(&bound).map(|()| LocalDeleteOutcome::new(true));
        }
        let result = { test_io_fault("local-fs-delete-directory-remove").map_or_else(|| fs::remove_dir(&bound), Err) };
        match result {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(source) if options.missing_ok() && source.kind() == io::ErrorKind::NotFound => {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(source) => Err(LocalFileError::from_io(
                LocalFileOperation::DeleteDirectory,
                Some(bound),
                None,
                source,
            )),
        }
    }
}

/// Removes a Host directory tree while tracking the first failed entry.
fn remove_host_directory_tree(path: &Path) -> LocalResult<()> {
    let mut removed_any = false;
    let mut work = vec![DeleteWork::Inspect(path.to_path_buf())];
    while let Some(item) = work.pop() {
        match item {
            DeleteWork::Inspect(current) => {
                let metadata = match fs::symlink_metadata(&current) {
                    Ok(metadata) => metadata,
                    Err(error) => return Err(delete_entry_error(&current, removed_any, error)),
                };
                if metadata.file_type().is_dir() {
                    let entries = match fs::read_dir(&current) {
                        Ok(entries) => entries,
                        Err(error) => return Err(delete_entry_error(&current, removed_any, error)),
                    };
                    let mut children = Vec::new();
                    for entry in entries {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(error) => {
                                return Err(delete_entry_error(&current, removed_any, error));
                            }
                        };
                        children.push(entry.path());
                    }
                    work.push(DeleteWork::RemoveDirectory(current));
                    for child in children.into_iter().rev() {
                        work.push(DeleteWork::Inspect(child));
                    }
                } else {
                    maybe_fail_host_delete(&current, removed_any)?;
                    if let Err(error) = remove_host_non_directory(&current, &metadata) {
                        return Err(delete_entry_error(&current, removed_any, error));
                    }
                    removed_any = true;
                }
            }
            DeleteWork::RemoveDirectory(current) => {
                maybe_fail_host_delete(&current, removed_any)?;
                if let Err(error) = fs::remove_dir(&current) {
                    return Err(delete_entry_error(&current, removed_any, error));
                }
                removed_any = true;
            }
        }
    }
    Ok(())
}

/// Removes one Host entry already known not to be a real directory.
fn remove_host_non_directory(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;

        if metadata.file_type().is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    let _ = metadata;
    fs::remove_file(path)
}

/// Injects the deterministic recursive-delete fault used by contract tests.
fn maybe_fail_host_delete(path: &Path, removed_any: bool) -> LocalResult<()> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support_on_nth("host-delete-directory-entry-second", 2) {
        return Err(delete_entry_error(path, removed_any, crate::local::test_fault_error()));
    }
    let _ = (path, removed_any);
    Ok(())
}

/// Builds one recursive-delete error with exact partial-publication state.
fn delete_entry_error(path: &Path, removed_any: bool, source: io::Error) -> LocalFileError {
    let error = LocalFileError::from_io(
        LocalFileOperation::DeleteDirectory,
        Some(path.to_path_buf()),
        None,
        source,
    );
    if removed_any {
        error.with_kind(LocalFileErrorKind::PublicationIncomplete)
    } else {
        error
    }
}

/// Reads final-entry metadata for a delete operation and handles missing
/// policy.
///
/// # Parameters
///
/// - `path`: Bound native path.
/// - `options`: Delete policy.
/// - `operation`: File or directory deletion operation.
///
/// # Returns
///
/// `Some` metadata for an existing entry or `None` for an accepted missing
/// entry.
///
/// # Errors
///
/// Returns `LocalFileError` when metadata inspection fails.
fn metadata_for_delete(
    path: &Path,
    options: &LocalDeleteOptions,
    operation: LocalFileOperation,
) -> LocalResult<Option<fs::Metadata>> {
    match test_io_fault("local-fs-delete-metadata").map_or_else(|| fs::symlink_metadata(path), Err) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && options.missing_ok() => Ok(None),
        Err(error) => Err(LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            error,
        )),
    }
}
