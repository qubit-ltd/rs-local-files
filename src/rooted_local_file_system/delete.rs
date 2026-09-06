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
use super::io;
use super::resolve_rooted_path;
use super::rooted_io_error;
use crate::local::DeleteBackend;
use crate::local::LocalRelativePath;
use crate::local::remove_directory_tree;

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
        let metadata = match self.root.symlink_metadata(&relative) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound && options.missing_ok() => {
                return Ok(LocalDeleteOutcome::new(false));
            }
            Err(error) => {
                return Err(rooted_io_error(LocalFileOperation::DeleteDirectory, path, error));
            }
        };
        if metadata.kind() != crate::rooted::EntryKind::Directory {
            return Err(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::DeleteDirectory)
                    .with_path(path.to_path_buf()),
            );
        }
        if options.recursive() {
            return match remove_directory_tree(self, &relative, *options) {
                Ok(()) => Ok(LocalDeleteOutcome::new(true)),
                Err(error)
                    if options.missing_ok()
                        && error.effect_state().is_none()
                        && error.cause_kind() == Some(LocalFileErrorKind::NotFound)
                        && error.path() == Some(relative.as_path()) =>
                {
                    Ok(LocalDeleteOutcome::new(false))
                }
                Err(error) => Err(error),
            };
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

impl DeleteBackend for RootedLocalFileSystem {
    type Path = LocalRelativePath;
    type Metadata = crate::rooted::Metadata;
    type Reader = crate::rooted::DirectoryReader;

    fn path<'a>(&self, value: &'a Self::Path) -> &'a std::path::Path {
        value.as_path()
    }

    fn metadata(&self, path: &Self::Path) -> io::Result<Self::Metadata> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support_on_nth("rooted-delete-directory-child-not-found", 2) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        self.root.symlink_metadata(path)
    }

    fn is_directory(&self, metadata: &Self::Metadata) -> bool {
        metadata.kind() == crate::rooted::EntryKind::Directory
    }

    fn open_directory(&self, path: &Self::Path) -> io::Result<Self::Reader> {
        self.root.open_dir_reader(path)
    }

    fn next_child(&self, parent: &Self::Path, reader: &mut Self::Reader) -> io::Result<Option<Self::Path>> {
        reader
            .next_entry()
            .and_then(|entry| entry.map_or(Ok(None), |entry| parent.join_component(entry.name()).map(Some)))
    }

    fn remove_non_directory(&self, path: &Self::Path, _metadata: &Self::Metadata) -> io::Result<()> {
        self.root.remove_file(path)
    }

    fn remove_empty_directory(&self, path: &Self::Path) -> io::Result<()> {
        self.root.remove_empty_dir(path)
    }

    fn before_remove(&self, path: &Self::Path) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support_on_nth("rooted-delete-directory-entry-second", 2) {
            return Err(crate::local::test_fault_error());
        }
        let _ = path;
        Ok(())
    }
}
