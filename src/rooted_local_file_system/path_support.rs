// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted-path validation and metadata conversion helpers.

use std::{
    io,
    path::{
        Path,
        PathBuf,
    },
};

use crate::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileKind,
    LocalFileMetadata,
    LocalFileOperation,
    LocalResult,
};

/// Validates a rooted descendant and preserves the offending native path.
///
/// # Errors
///
/// Returns `LocalFileError` for empty, absolute, prefixed, dot, or parent
/// paths.
#[inline]
pub(super) fn rooted_path(
    path: &Path,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    crate::local::LocalRelativePath::new(path).map_err(|error| {
        LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            error,
        )
    })
}

/// Reports whether a rooted destination currently names a real directory.
///
/// # Errors
///
/// Returns native metadata errors other than an absent final entry.
pub(super) fn rooted_destination_is_directory(
    root: &crate::rooted::Root,
    path: &crate::local::LocalRelativePath,
) -> io::Result<bool> {
    match root.symlink_metadata(path) {
        Ok(metadata) => {
            Ok(metadata.kind() == crate::rooted::EntryKind::Directory)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Validates an optional rooted temporary-resource parent.
///
/// # Errors
///
/// Returns `LocalFileError` when the configured parent is not a normal
/// relative descendant of the opened root.
#[inline]
pub(super) fn rooted_temp_parent(
    parent: Option<&Path>,
    operation: LocalFileOperation,
) -> LocalResult<PathBuf> {
    parent.map_or_else(
        || Ok(PathBuf::new()),
        |parent| {
            rooted_path(parent, operation)
                .map(|path| path.as_path().to_path_buf())
        },
    )
}

/// Confirms that a rooted temporary-resource parent is an existing directory.
///
/// # Errors
///
/// Returns `LocalFileError` when the parent cannot be read or is not a
/// directory.
#[inline]
pub(super) fn validate_rooted_temp_parent(
    root: &crate::rooted::Root,
    parent: &Path,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let relative =
        crate::local::LocalRelativePath::new(parent).map_err(|error| {
            rooted_io_error(operation, parent, error)
                .with_kind(LocalFileErrorKind::InvalidPath)
        })?;
    let metadata = root
        .symlink_metadata(&relative)
        .map_err(|error| rooted_io_error(operation, parent, error))?;
    if metadata.kind() != crate::rooted::EntryKind::Directory {
        return Err(LocalFileError::new(
            LocalFileErrorKind::NotDirectory,
            operation,
        )
        .with_path(parent.to_path_buf()));
    }
    Ok(())
}

/// Generates one rooted temporary-entry candidate beneath a validated parent.
///
/// # Errors
///
/// Returns `LocalFileError` when an affix is invalid or randomness is
/// unavailable.
#[inline]
pub(super) fn temp_candidate(
    parent: &Path,
    prefix: Option<&str>,
    suffix: Option<&str>,
    operation: LocalFileOperation,
) -> LocalResult<PathBuf> {
    crate::local::try_random_file_name("qubit-local-files-", prefix, suffix)
        .map(|name| parent.join(name))
        .map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = rooted_io_error(operation, parent, error);
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })
}

/// Converts descriptor-relative metadata to the unified metadata type.
#[inline]
pub(crate) fn rooted_metadata(
    metadata: crate::rooted::Metadata,
) -> LocalFileMetadata {
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
    LocalFileMetadata::from_parts(
        kind,
        metadata.size(),
        metadata.accessed_at(),
        metadata.modified_at(),
        metadata.created_at(),
    )
}

/// Adds rooted operation and descendant context to a native I/O failure.
#[inline(always)]
pub(super) fn rooted_io_error(
    operation: LocalFileOperation,
    path: &Path,
    error: io::Error,
) -> LocalFileError {
    LocalFileError::from_io(operation, Some(path.to_path_buf()), None, error)
}
