// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rename publication-state constructors.

use std::io;
use std::path::Path;

use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalRenameFailure;
use crate::LocalRenameFailureState;

/// Wraps a pre-publication rename error.
///
/// The returned failure proves that the destination namespace is unchanged.
#[inline(always)]
pub(crate) fn rename_failure_unchanged(
    error: LocalFileError,
) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Unchanged)
}

/// Wraps a rename error after publication completed.
///
/// The returned failure records that the destination was renamed before the
/// subsequent error occurred.
#[inline]
pub(crate) fn rename_failure_renamed(
    error: LocalFileError,
) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Renamed)
}

/// Maps a native rename failure to the strongest state guaranteed by its
/// operation contract.
///
/// # Parameters
///
/// - `source`: Native source path retained in the typed error.
/// - `target`: Native destination path retained in the typed error.
/// - `error`: Native rename failure to classify.
///
/// # Returns
///
/// A typed rename failure with the strongest proven publication state.
#[inline]
pub(crate) fn rename_failure_after_native_attempt(
    source: &Path,
    target: &Path,
    error: io::Error,
) -> LocalRenameFailure {
    let state = match error.kind() {
        io::ErrorKind::AlreadyExists
        | io::ErrorKind::CrossesDevices
        | io::ErrorKind::NotFound => LocalRenameFailureState::Unchanged,
        _ => LocalRenameFailureState::Indeterminate,
    };
    LocalRenameFailure::new(
        LocalFileError::from_io(
            LocalFileOperation::Rename,
            Some(source.to_path_buf()),
            Some(target.to_path_buf()),
            error,
        ),
        state,
    )
}
