// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host delete operations.
// qubit-style: allow source-test-pair

use std::time::Instant;

use super::HostLocalFileSystem;
use super::LocalDeleteOptions;
use super::LocalDeleteOutcome;
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
use crate::local::DeleteBackend;
use crate::local::remove_directory_tree;

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
                LocalFileError::new(LocalFileErrorKind::IsDirectory, LocalFileOperation::DeleteFile).with_path(bound),
            );
        }
        match test_io_fault("local-fs-delete-file-remove")
            .map_or_else(|| remove_host_non_directory(&bound, &metadata), Err)
        {
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
        started_at: Instant,
    ) -> LocalResult<LocalDeleteOutcome> {
        let bound = resolve_host_path(path, symlink_policy, false)?;
        let Some(metadata) = metadata_for_delete(&bound, options, LocalFileOperation::DeleteDirectory)? else {
            return Ok(LocalDeleteOutcome::new(false));
        };
        if !metadata.file_type().is_dir() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::DeleteDirectory)
                    .with_path(bound),
            );
        }
        if options.recursive() {
            return match remove_directory_tree(&HostLocalFileSystem { _private: () }, &bound, *options, started_at) {
                Ok(()) => Ok(LocalDeleteOutcome::new(true)),
                Err(error)
                    if options.missing_ok()
                        && error.effect_state().is_none()
                        && error.cause_kind() == Some(LocalFileErrorKind::NotFound)
                        && error.path() == Some(bound.as_path()) =>
                {
                    Ok(LocalDeleteOutcome::new(false))
                }
                Err(error) => Err(error),
            };
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

/// Removes one Host entry already known not to be a real directory.
pub(crate) fn remove_host_non_directory(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
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

impl DeleteBackend for HostLocalFileSystem {
    /// Bound native coordinates for queued deletion work.
    type Path = PathBuf;
    /// No-follow metadata used to distinguish directories from removable
    /// leaves.
    type Metadata = fs::Metadata;
    /// Lazy native directory reader owned by the deletion scheduler.
    type Reader = fs::ReadDir;

    /// Borrows diagnostic coordinates without allocating or resolving the
    /// entry.
    #[inline]
    fn path<'a>(&self, value: &'a Self::Path) -> &'a Path {
        value
    }

    /// Inspects the final entry without following its symbolic-link target.
    ///
    /// # Errors
    ///
    /// Returns native inspection errors, including disappearance during
    /// traversal.
    fn metadata(&self, path: &Self::Path) -> io::Result<Self::Metadata> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support_on_nth("host-delete-directory-child-not-found", 2) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        fs::symlink_metadata(path)
    }

    /// Tests whether inspected metadata denotes a real directory rather than a
    /// link.
    #[inline]
    fn is_directory(&self, metadata: &Self::Metadata) -> bool {
        metadata.file_type().is_dir()
    }

    /// Opens a lazy reader whose lifetime is controlled by the scheduler.
    ///
    /// # Errors
    ///
    /// Returns native directory-open errors without removing any entries.
    #[inline]
    fn open_directory(&self, path: &Self::Path) -> io::Result<Self::Reader> {
        fs::read_dir(path)
    }

    /// Produces one child coordinate, or `None` when the reader is exhausted.
    ///
    /// # Errors
    ///
    /// Returns enumeration failures.
    fn next_child(&self, _parent: &Self::Path, reader: &mut Self::Reader) -> io::Result<Option<Self::Path>> {
        reader.next().transpose().map(|entry| entry.map(|entry| entry.path()))
    }

    /// Removes the inspected leaf itself, including a final directory link.
    ///
    /// # Errors
    ///
    /// Returns native unlink errors; symbolic-link targets remain untouched.
    #[inline]
    fn remove_non_directory(&self, path: &Self::Path, metadata: &Self::Metadata) -> io::Result<()> {
        remove_host_non_directory(path, metadata)
    }

    /// Removes a directory after the scheduler has processed its children.
    ///
    /// # Errors
    ///
    /// Returns native removal errors, including concurrent child creation.
    #[inline]
    fn remove_empty_directory(&self, path: &Self::Path) -> io::Result<()> {
        fs::remove_dir(path)
    }

    /// Runs the deterministic test-support fault boundary before native
    /// removal.
    ///
    /// Production calls have no side effects; an enabled test fault returns an
    /// I/O error before the scheduler can count this entry as removed.
    fn before_remove(&self, path: &Self::Path) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support_on_nth("host-delete-directory-entry-second", 2) {
            return Err(crate::local::test_fault_error());
        }
        let _ = path;
        Ok(())
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
