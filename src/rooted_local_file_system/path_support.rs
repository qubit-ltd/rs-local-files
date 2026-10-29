// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted-path validation and metadata conversion helpers.

use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFilePermissions;
use crate::LocalResult;

/// Validates a rooted descendant and preserves the offending native path.
///
/// # Parameters
///
/// * `path` - Candidate descendant relative to the opened root.
/// * `operation` - Operation recorded in any validation error.
///
/// # Returns
///
/// The validated relative path on success.
///
/// # Errors
///
/// Returns `LocalFileError` for empty, absolute, prefixed, dot, parent, or
/// otherwise invalid rooted paths. The error retains the candidate path and
/// operation context.
#[inline]
pub(crate) fn rooted_path(path: &Path, operation: LocalFileOperation) -> LocalResult<crate::local::LocalRelativePath> {
    match crate::local::LocalRelativePath::new(path) {
        Ok(path) => Ok(path),
        Err(error) => Err(LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            error,
        )),
    }
}

/// Reports whether a rooted destination currently names a real directory.
///
/// # Parameters
///
/// * `root` - Open directory capability used for descriptor-relative lookup.
/// * `path` - Validated descendant to inspect without following its final link.
///
/// # Returns
///
/// `true` only when the current entry is a directory; an absent entry returns
/// `false`.
///
/// # Errors
///
/// Returns native metadata errors other than an absent final entry, preserving
/// the operating-system error for the caller to add operation context.
pub(crate) fn rooted_destination_is_directory(
    root: &crate::rooted::Root,
    path: &crate::local::LocalRelativePath,
) -> io::Result<bool> {
    match root.symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.kind() == crate::rooted::EntryKind::Directory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Validates an optional rooted temporary-resource parent.
///
/// # Parameters
///
/// * `parent` - Optional configured parent path; absence or an empty path means
///   the opened root itself.
/// * `operation` - Operation recorded in any validation error.
///
/// # Returns
///
/// An owned relative parent path, empty when the root itself is selected.
///
/// # Errors
///
/// Returns `LocalFileError` when the configured parent is not a normal
/// relative descendant of the opened root; the error retains the operation
/// and offending path.
#[inline]
pub(crate) fn rooted_temp_parent(parent: Option<&Path>, operation: LocalFileOperation) -> LocalResult<PathBuf> {
    let Some(parent) = parent else {
        return Ok(PathBuf::new());
    };
    if parent.as_os_str().is_empty() {
        Ok(PathBuf::new())
    } else {
        let path = rooted_path(parent, operation)?;
        Ok(path.as_path().to_path_buf())
    }
}

/// Confirms that a rooted temporary-resource parent is an existing directory.
///
/// # Parameters
///
/// * `root` - Open directory capability that bounds the lookup.
/// * `parent` - Relative parent path; an empty path denotes the root itself.
/// * `operation` - Operation recorded in any validation error.
///
/// # Returns
///
/// `()` when the parent is empty or resolves to a directory.
///
/// # Errors
///
/// Returns `LocalFileError` when the path is invalid, cannot be inspected, or
/// resolves to a non-directory entry. Native lookup context is retained.
#[inline]
pub(crate) fn validate_rooted_temp_parent(
    root: &crate::rooted::Root,
    parent: &Path,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let relative = match crate::local::LocalRelativePath::new(parent) {
        Ok(relative) => relative,
        Err(error) => {
            return Err(rooted_io_error(operation, parent, error).with_kind(LocalFileErrorKind::InvalidPath));
        }
    };
    let metadata = match root.symlink_metadata(&relative) {
        Ok(metadata) => metadata,
        Err(error) => return Err(rooted_io_error(operation, parent, error)),
    };
    if metadata.kind() != crate::rooted::EntryKind::Directory {
        return Err(LocalFileError::new(LocalFileErrorKind::NotDirectory, operation).with_path(parent.to_path_buf()));
    }
    Ok(())
}

/// Generates one rooted temporary-entry candidate beneath a validated parent.
///
/// # Parameters
///
/// * `parent` - Validated directory path beneath which to place the candidate.
/// * `prefix` - Optional filename prefix.
/// * `suffix` - Optional filename suffix.
/// * `operation` - Operation recorded in any generation error.
///
/// # Returns
///
/// A candidate path formed by joining the parent and generated filename.
///
/// # Errors
///
/// Returns `LocalFileError` when an affix is invalid or randomness is
/// unavailable; invalid affixes are classified as invalid options.
#[inline]
pub(crate) fn temp_candidate(
    parent: &Path,
    prefix: Option<&str>,
    suffix: Option<&str>,
    operation: LocalFileOperation,
) -> LocalResult<PathBuf> {
    match crate::local::try_random_file_name("qubit-local-files-", prefix, suffix) {
        Ok(name) => Ok(parent.join(name)),
        Err(error) => {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = rooted_io_error(operation, parent, error);
            if invalid_options {
                Err(error.with_kind(LocalFileErrorKind::InvalidOptions))
            } else {
                Err(error)
            }
        }
    }
}

/// Converts descriptor-relative metadata to the unified metadata type.
///
/// # Parameters
///
/// * `metadata` - Metadata obtained from a rooted descriptor-relative lookup.
///
/// # Returns
///
/// Unified local metadata with the entry kind, size, timestamps, and
/// permissions copied from the rooted representation.
#[inline]
pub(crate) fn rooted_metadata(metadata: crate::rooted::Metadata) -> LocalFileMetadata {
    let kind = match metadata.kind() {
        crate::rooted::EntryKind::File => LocalFileKind::File,
        crate::rooted::EntryKind::Directory => LocalFileKind::Directory,
        crate::rooted::EntryKind::Symlink => LocalFileKind::Symlink,
        #[cfg(unix)]
        crate::rooted::EntryKind::Fifo => LocalFileKind::Fifo,
        #[cfg(unix)]
        crate::rooted::EntryKind::Socket => LocalFileKind::Socket,
        #[cfg(unix)]
        crate::rooted::EntryKind::BlockDevice => LocalFileKind::BlockDevice,
        #[cfg(unix)]
        crate::rooted::EntryKind::CharDevice => LocalFileKind::CharDevice,
        crate::rooted::EntryKind::Other => LocalFileKind::Other,
    };
    let permissions = metadata.permissions();
    LocalFileMetadata::from_parts(
        kind,
        metadata.size(),
        metadata.accessed_at(),
        metadata.modified_at(),
        metadata.created_at(),
        LocalFilePermissions::new(permissions.is_read_only(), permissions.unix_mode()),
    )
}

/// Adds rooted operation and descendant context to a native I/O failure.
///
/// # Parameters
///
/// * `operation` - Filesystem operation that encountered the failure.
/// * `path` - Root-relative path associated with the failure.
/// * `error` - Native I/O error whose kind and message are retained.
///
/// # Returns
///
/// A `LocalFileError` containing the operation and path context alongside the
/// original I/O failure.
#[inline]
pub(crate) fn rooted_io_error(operation: LocalFileOperation, path: &Path, error: io::Error) -> LocalFileError {
    LocalFileError::from_io(operation, Some(path.to_path_buf()), None, error)
}

#[cfg(test)]
mod invalid_parent_and_affix_tests {
    use std::path::Path;

    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::rooted::Root;

    #[test]
    fn test_rooted_temp_parent_rejects_parent_traversal_with_original_context() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        let root = Root::open(fixture.path()).expect("root authority should open");
        let path = Path::new("../outside");

        let error = super::validate_rooted_temp_parent(&root, path, LocalFileOperation::Metadata)
            .expect_err("temporary parent must stay within root");

        assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.path(), Some(path));
    }

    #[test]
    fn test_rooted_temp_candidate_classifies_invalid_affix_as_invalid_options() {
        let parent = Path::new("temp-parent");
        for (prefix, suffix) in [(Some("bad/prefix"), None), (None, Some("bad/suffix"))] {
            let error = super::temp_candidate(parent, prefix, suffix, LocalFileOperation::Metadata)
                .expect_err("an affix may not introduce additional path components");

            assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
            assert_eq!(error.operation(), LocalFileOperation::Metadata);
            assert_eq!(error.path(), Some(parent));
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::InvalidInput);
        }
    }
}
