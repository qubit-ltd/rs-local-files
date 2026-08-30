// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted directory operations.
// qubit-style: allow source-test-pair

use super::LocalCreateDirectoryOptions;
use super::LocalCreateDirectoryOutcome;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::RootedLocalFileSystem;
use super::io;
use super::resolve_rooted_path;
use super::rooted_io_error;

impl RootedLocalFileSystem {
    /// Creates a directory below the opened root.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative descendant path.
    /// - `options`: Ancestor creation policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether the requested entry was newly created.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for lexical escape, symlink traversal, type
    /// conflicts, or native creation failures.
    pub fn create_directory(
        &self,
        path: &Path,
        options: &LocalCreateDirectoryOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            false,
            LocalFileOperation::CreateDirectory,
        )?;
        #[cfg(feature = "internal-test-support")]
        let metadata = if crate::local::test_support_enabled("rooted-local-create-directory-status") {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        } else {
            self.root.symlink_metadata(&relative)
        };
        #[cfg(not(feature = "internal-test-support"))]
        let metadata = self.root.symlink_metadata(&relative);
        let existing_directory = match metadata {
            Ok(metadata) if metadata.kind() == crate::rooted::EntryKind::Directory => Some(true),
            Ok(_) => {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::CreateDirectory)
                        .with_path(path.to_path_buf()),
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(rooted_io_error(LocalFileOperation::CreateDirectory, path, error));
            }
        };
        let existed = existing_directory.is_some();
        if existed && !options.exists_ok() {
            return Err(rooted_io_error(
                LocalFileOperation::CreateDirectory,
                path,
                io::Error::from(io::ErrorKind::AlreadyExists),
            ));
        }
        if existing_directory == Some(true) {
            return Ok(LocalCreateDirectoryOutcome::new(false));
        }
        let result = if options.recursive() {
            self.root.create_dir_all(&relative)
        } else {
            self.root.create_dir(&relative)
        };
        match result {
            Ok(()) => Ok(LocalCreateDirectoryOutcome::new(!existed)),
            Err(error)
                if options.exists_ok()
                    && error.kind() == io::ErrorKind::AlreadyExists
                    && self
                        .root
                        .symlink_metadata(&relative)
                        .is_ok_and(|metadata| metadata.kind() == crate::rooted::EntryKind::Directory) =>
            {
                Ok(LocalCreateDirectoryOutcome::new(false))
            }
            Err(error) => Err(rooted_io_error(LocalFileOperation::CreateDirectory, path, error)),
        }
    }
}
