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
use super::PathBuf;
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
        if options.recursive() {
            return create_rooted_directory_tree(&self.root, &relative, options.exists_ok())
                .map(LocalCreateDirectoryOutcome::new);
        }
        let result = self.root.create_dir(&relative);
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

/// Creates each missing Rooted component through the opened authority.
fn create_rooted_directory_tree(
    root: &crate::rooted::Root,
    path: &crate::local::LocalRelativePath,
    exists_ok: bool,
) -> LocalResult<bool> {
    let mut current = PathBuf::new();
    let mut created_any = false;
    let mut created_target = false;
    for component in path.as_path().components() {
        current.push(component.as_os_str());
        let current =
            crate::local::LocalRelativePath::new(&current).expect("prefixes of a validated rooted path remain valid");
        match root.symlink_metadata(&current) {
            Ok(metadata) if metadata.kind() == crate::rooted::EntryKind::Directory => {
                if current.as_path() == path.as_path() && !exists_ok {
                    return Err(rooted_create_component_error(
                        &current,
                        created_any,
                        io::Error::from(io::ErrorKind::AlreadyExists),
                    ));
                }
            }
            Ok(_) => {
                return Err(rooted_create_component_error(
                    &current,
                    created_any,
                    io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "rooted directory path component is not a directory",
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                #[cfg(feature = "internal-test-support")]
                if crate::local::take_test_support_on_nth("rooted-create-directory-component-second", 2) {
                    return Err(rooted_create_component_error(
                        &current,
                        created_any,
                        crate::local::test_fault_error(),
                    ));
                }
                match root.create_dir(&current) {
                    Ok(()) => {
                        created_any = true;
                        created_target = current.as_path() == path.as_path();
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let raced_directory = root
                            .symlink_metadata(&current)
                            .is_ok_and(|metadata| metadata.kind() == crate::rooted::EntryKind::Directory);
                        if !raced_directory || (current.as_path() == path.as_path() && !exists_ok) {
                            return Err(rooted_create_component_error(&current, created_any, error));
                        }
                    }
                    Err(error) => {
                        return Err(rooted_create_component_error(&current, created_any, error));
                    }
                }
            }
            Err(error) => {
                return Err(rooted_create_component_error(&current, created_any, error));
            }
        }
    }
    Ok(created_target)
}

/// Builds one Rooted recursive-create error with its failed relative path.
fn rooted_create_component_error(
    path: &crate::local::LocalRelativePath,
    created_any: bool,
    source: io::Error,
) -> LocalFileError {
    let error = LocalFileError::from_io(
        LocalFileOperation::CreateDirectory,
        Some(path.as_path().to_path_buf()),
        None,
        source,
    );
    if created_any {
        error.with_kind(LocalFileErrorKind::PublicationIncomplete)
    } else {
        error
    }
}
