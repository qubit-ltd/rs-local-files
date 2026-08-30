// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host directory operations.
// qubit-style: allow source-test-pair

use super::HostLocalFileSystem;
use super::LocalCreateDirectoryOptions;
use super::LocalCreateDirectoryOutcome;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::fs;
use super::io;
use super::resolve_host_path;
use super::test_io_fault;

impl HostLocalFileSystem {
    /// Creates a Host directory using an explicit symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative directory path.
    /// - `options`: Directory creation policy.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether the requested entry was newly created.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when creation fails or an existing entry is not
    /// a directory.
    pub fn create_directory_with_policy(
        path: &Path,
        options: &LocalCreateDirectoryOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let bound = resolve_host_path(path, symlink_policy, false)?;
        let existing_directory = match test_io_fault("local-fs-create-directory-exists")
            .map_or_else(|| fs::symlink_metadata(&bound), Err)
        {
            Ok(metadata) if metadata.file_type().is_dir() => Some(true),
            Ok(_) => {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::CreateDirectory)
                        .with_path(bound),
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::CreateDirectory,
                    Some(bound),
                    None,
                    source,
                ));
            }
        };
        let existed = existing_directory.is_some();
        if existed && !options.exists_ok() {
            return Err(LocalFileError::from_io(
                LocalFileOperation::CreateDirectory,
                Some(bound),
                None,
                io::Error::from(io::ErrorKind::AlreadyExists),
            ));
        }
        if existing_directory == Some(true) {
            return Ok(LocalCreateDirectoryOutcome::new(false));
        }
        let result = if options.recursive() {
            fs::create_dir_all(&bound)
        } else {
            fs::create_dir(&bound)
        };
        match result {
            Ok(()) => Ok(LocalCreateDirectoryOutcome::new(!existed)),
            Err(source)
                if options.exists_ok()
                    && source.kind() == io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&bound).is_ok_and(|metadata| metadata.file_type().is_dir()) =>
            {
                Ok(LocalCreateDirectoryOutcome::new(false))
            }
            Err(source) => Err(LocalFileError::from_io(
                LocalFileOperation::CreateDirectory,
                Some(bound),
                None,
                source,
            )),
        }
    }
}
