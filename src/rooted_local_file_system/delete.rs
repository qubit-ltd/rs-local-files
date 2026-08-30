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
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::RootedLocalFileSystem;
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
        let result = if options.recursive() {
            self.root.remove_tree(&relative)
        } else {
            self.root.remove_empty_dir(&relative)
        };
        match result {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(error) if error.kind() == io::ErrorKind::NotFound && options.missing_ok() => {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(error) => Err(rooted_io_error(LocalFileOperation::DeleteDirectory, path, error)),
        }
    }
}
