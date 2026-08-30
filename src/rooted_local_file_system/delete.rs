// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted delete operations.
// qubit-style: allow source-test-pair

use super::LocalDeleteOptions;
use super::LocalDeleteOutcome;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::RootedLocalFileSystem;
use super::delete_work::DeleteWork;
use super::io;
use super::resolve_rooted_path;
use super::rooted_io_error;

impl RootedLocalFileSystem {
    /// Deletes a rooted file or final symbolic-link entry.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative entry path.
    /// - `options`: Missing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether an entry was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, directory type
    /// conflicts, or native removal failures.
    pub fn delete_file(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let relative = resolve_rooted_path(&self.root, path, symlink_policy, false, LocalFileOperation::DeleteFile)?;
        let result = self.root.remove_file(&relative);
        match result {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(error) if error.kind() == io::ErrorKind::NotFound && options.missing_ok() => {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(error) => Err(rooted_io_error(LocalFileOperation::DeleteFile, path, error)),
        }
    }

    /// Deletes a rooted directory without following a final link.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative directory path.
    /// - `options`: Recursion and missing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether a directory was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, type conflicts, or
    /// native removal failures.
    pub fn delete_directory(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            false,
            LocalFileOperation::DeleteDirectory,
        )?;
        if options.recursive() {
            return remove_rooted_directory_tree(&self.root, &relative)
                .map(|()| LocalDeleteOutcome::new(true))
                .or_else(|error| {
                    if error.kind() == LocalFileErrorKind::NotFound && options.missing_ok() {
                        Ok(LocalDeleteOutcome::new(false))
                    } else {
                        Err(error)
                    }
                });
        }
        let result = self.root.remove_empty_dir(&relative);
        match result {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(error) if error.kind() == io::ErrorKind::NotFound && options.missing_ok() => {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(error) => Err(rooted_io_error(LocalFileOperation::DeleteDirectory, path, error)),
        }
    }
}

/// Removes a Rooted directory tree and retains its first failed entry.
fn remove_rooted_directory_tree(root: &crate::rooted::Root, path: &crate::local::LocalRelativePath) -> LocalResult<()> {
    let mut removed_any = false;
    let mut work = vec![DeleteWork::Inspect(path.clone())];
    while let Some(item) = work.pop() {
        match item {
            DeleteWork::Inspect(current) => {
                let metadata = root
                    .symlink_metadata(&current)
                    .map_err(|error| rooted_delete_entry_error(&current, removed_any, error))?;
                if metadata.kind() == crate::rooted::EntryKind::Directory {
                    let entries = root
                        .read_dir(&current)
                        .map_err(|error| rooted_delete_entry_error(&current, removed_any, error))?;
                    work.push(DeleteWork::RemoveDirectory(current.clone()));
                    for entry in entries.into_iter().rev() {
                        let child = current
                            .join_component(entry.name())
                            .expect("rooted directory names are normal components");
                        work.push(DeleteWork::Inspect(child));
                    }
                } else {
                    maybe_fail_rooted_delete(&current, removed_any)?;
                    root.remove_file(&current)
                        .map_err(|error| rooted_delete_entry_error(&current, removed_any, error))?;
                    removed_any = true;
                }
            }
            DeleteWork::RemoveDirectory(current) => {
                maybe_fail_rooted_delete(&current, removed_any)?;
                root.remove_empty_dir(&current)
                    .map_err(|error| rooted_delete_entry_error(&current, removed_any, error))?;
                removed_any = true;
            }
        }
    }
    Ok(())
}

/// Injects the deterministic Rooted recursive-delete contract-test fault.
fn maybe_fail_rooted_delete(path: &crate::local::LocalRelativePath, removed_any: bool) -> LocalResult<()> {
    #[cfg(feature = "internal-test-support")]
    if crate::local::take_test_support_on_nth("rooted-delete-directory-entry-second", 2) {
        return Err(rooted_delete_entry_error(
            path,
            removed_any,
            crate::local::test_fault_error(),
        ));
    }
    let _ = (path, removed_any);
    Ok(())
}

/// Builds one Rooted recursive-delete error with its failed relative path.
fn rooted_delete_entry_error(
    path: &crate::local::LocalRelativePath,
    removed_any: bool,
    source: io::Error,
) -> LocalFileError {
    let error = LocalFileError::from_io(
        LocalFileOperation::DeleteDirectory,
        Some(path.as_path().to_path_buf()),
        None,
        source,
    );
    if removed_any {
        error.with_kind(LocalFileErrorKind::PublicationIncomplete)
    } else {
        error
    }
}
