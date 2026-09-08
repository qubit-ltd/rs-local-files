// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted delete operations.
// qubit-style: allow source-test-pair

use std::time::Instant;

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
        started_at: Instant,
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
            return match remove_directory_tree(self, &relative, *options, started_at) {
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
    /// Validated root-relative coordinates for queued deletion work.
    type Path = LocalRelativePath;
    /// No-follow metadata used to distinguish directories from removable
    /// leaves.
    type Metadata = crate::rooted::Metadata;
    /// Lazy native directory reader owned by the deletion scheduler.
    type Reader = crate::rooted::DirectoryReader;

    /// Borrows diagnostic coordinates without allocating or resolving the
    /// entry.
    #[inline(always)]
    fn path<'a>(&self, value: &'a Self::Path) -> &'a std::path::Path {
        value.as_path()
    }

    /// Inspects the final entry without following its symbolic-link target.
    ///
    /// # Errors
    ///
    /// Returns native inspection errors, including disappearance during
    /// traversal.
    fn metadata(&self, path: &Self::Path) -> io::Result<Self::Metadata> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support_on_nth("rooted-delete-directory-child-not-found", 2) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        self.root.symlink_metadata(path)
    }

    /// Tests whether inspected metadata denotes a real directory rather than a
    /// link.
    #[inline(always)]
    fn is_directory(&self, metadata: &Self::Metadata) -> bool {
        metadata.kind() == crate::rooted::EntryKind::Directory
    }

    /// Opens a lazy reader whose lifetime is controlled by the scheduler.
    ///
    /// # Errors
    ///
    /// Returns native directory-open errors without removing any entries.
    #[inline(always)]
    fn open_directory(&self, path: &Self::Path) -> io::Result<Self::Reader> {
        self.root.open_dir_reader(path)
    }

    /// Produces one child coordinate, or `None` when the reader is exhausted.
    ///
    /// # Errors
    ///
    /// Returns enumeration or relative-coordinate validation failures.
    fn next_child(&self, parent: &Self::Path, reader: &mut Self::Reader) -> io::Result<Option<Self::Path>> {
        reader
            .next_entry()
            .and_then(|entry| entry.map_or(Ok(None), |entry| parent.join_component(entry.name()).map(Some)))
    }

    /// Removes the inspected leaf itself, including a final directory link.
    ///
    /// # Errors
    ///
    /// Returns native unlink errors; symbolic-link targets remain untouched.
    #[inline(always)]
    fn remove_non_directory(&self, path: &Self::Path, _metadata: &Self::Metadata) -> io::Result<()> {
        self.root.remove_file(path)
    }

    /// Removes a directory after the scheduler has processed its children.
    ///
    /// # Errors
    ///
    /// Returns native removal errors, including concurrent child creation.
    #[inline(always)]
    fn remove_empty_directory(&self, path: &Self::Path) -> io::Result<()> {
        self.root.remove_empty_dir(path)
    }

    /// Runs the deterministic test-support fault boundary before native
    /// removal.
    ///
    /// Production calls have no side effects; an enabled test fault returns an
    /// I/O error before the scheduler can count this entry as removed.
    fn before_remove(&self, path: &Self::Path) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support_on_nth("rooted-delete-directory-entry-second", 2) {
            return Err(crate::local::test_fault_error());
        }
        let _ = path;
        Ok(())
    }
}
