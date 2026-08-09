// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host delete operations.
// qubit-style: allow source-test-pair

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
        let Some(metadata) = metadata_for_delete(
            &bound,
            options,
            LocalFileOperation::DeleteFile,
        )?
        else {
            return Ok(LocalDeleteOutcome::new(false));
        };
        if metadata.file_type().is_dir() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::DeleteFile,
            )
            .with_path(bound));
        }
        match test_io_fault("local-fs-delete-file-remove")
            .map_or_else(|| fs::remove_file(&bound), Err)
        {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(source)
                if options.missing_ok()
                    && source.kind() == io::ErrorKind::NotFound =>
            {
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
        let Some(metadata) = metadata_for_delete(
            &bound,
            options,
            LocalFileOperation::DeleteDirectory,
        )?
        else {
            return Ok(LocalDeleteOutcome::new(false));
        };
        if !metadata.file_type().is_dir() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::DeleteDirectory,
            )
            .with_path(bound));
        }
        let result = if options.recursive() {
            test_io_fault("local-fs-delete-directory-remove")
                .map_or_else(|| fs::remove_dir_all(&bound), Err)
        } else {
            test_io_fault("local-fs-delete-directory-remove")
                .map_or_else(|| fs::remove_dir(&bound), Err)
        };
        match result {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(source)
                if options.missing_ok()
                    && source.kind() == io::ErrorKind::NotFound =>
            {
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
    match test_io_fault("local-fs-delete-metadata")
        .map_or_else(|| fs::symlink_metadata(path), Err)
    {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && options.missing_ok() =>
        {
            Ok(None)
        }
        Err(error) => Err(LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            error,
        )),
    }
}
