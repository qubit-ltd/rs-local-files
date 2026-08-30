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
use super::PathBuf;
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
        if options.recursive() {
            return create_host_directory_tree(&bound, options.exists_ok()).map(LocalCreateDirectoryOutcome::new);
        }
        let result = fs::create_dir(&bound);
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

/// Creates each missing Host path component in shallow-to-deep order.
///
/// Returning the exact failed component lets the public facade distinguish an
/// unchanged failure from a non-transactional partial publication.
fn create_host_directory_tree(path: &Path, exists_ok: bool) -> LocalResult<bool> {
    let mut current = PathBuf::new();
    let mut created_any = false;
    let mut created_target = false;
    for component in path.components() {
        current.push(component.as_os_str());
        if !matches!(component, std::path::Component::Normal(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                if current == path && !exists_ok {
                    return Err(create_component_error(
                        &current,
                        created_any,
                        io::Error::from(io::ErrorKind::AlreadyExists),
                    ));
                }
            }
            Ok(_) => {
                return Err(create_component_error(
                    &current,
                    created_any,
                    io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "directory path component is not a directory",
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                #[cfg(feature = "internal-test-support")]
                if crate::local::take_test_support_on_nth("host-create-directory-component-second", 2) {
                    return Err(create_component_error(
                        &current,
                        created_any,
                        crate::local::test_fault_error(),
                    ));
                }
                match fs::create_dir(&current) {
                    Ok(()) => {
                        created_any = true;
                        created_target = current == path;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let raced_directory =
                            fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_dir());
                        if !raced_directory || (current == path && !exists_ok) {
                            return Err(create_component_error(&current, created_any, error));
                        }
                    }
                    Err(error) => {
                        return Err(create_component_error(&current, created_any, error));
                    }
                }
            }
            Err(error) => {
                return Err(create_component_error(&current, created_any, error));
            }
        }
    }
    Ok(created_target)
}

/// Builds one recursive-create error while retaining partial publication.
fn create_component_error(path: &Path, created_any: bool, source: io::Error) -> LocalFileError {
    let error = LocalFileError::from_io(
        LocalFileOperation::CreateDirectory,
        Some(path.to_path_buf()),
        None,
        source,
    );
    if created_any {
        error.with_kind(LocalFileErrorKind::PublicationIncomplete)
    } else {
        error
    }
}
