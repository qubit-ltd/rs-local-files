// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared publication-state translation for host and rooted operations.

use std::{
    io,
    path::Path,
};

use crate::{
    LocalCopyFailure,
    LocalCopyFailureState,
    LocalCopyStats,
    LocalDurabilityRequirement,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalRenameFailure,
    LocalRenameFailureState,
    LocalResult,
};

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
#[inline(always)]
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

/// Wraps a pre-publication copy error with an unchanged destination state.
///
/// # Parameters
///
/// - `error`: Typed copy error raised before destination publication.
///
/// # Returns
///
/// A copy failure with empty partial statistics and unchanged state.
#[inline]
pub(crate) fn copy_failure_unchanged(error: LocalFileError) -> LocalCopyFailure {
    LocalCopyFailure::new(
        error,
        LocalCopyFailureState::Unchanged,
        LocalCopyStats::default(),
        None,
        None,
    )
}

/// Wraps a post-publication copy error with partial statistics.
///
/// # Parameters
///
/// - `error`: Typed error raised after destination publication.
/// - `partial_stats`: Statistics confirmed before the error.
///
/// # Returns
///
/// A copy failure marked as published with the supplied statistics.
#[inline]
pub(crate) fn copy_failure_published(
    error: LocalFileError,
    partial_stats: LocalCopyStats,
) -> LocalCopyFailure {
    LocalCopyFailure::new(
        error,
        LocalCopyFailureState::Published,
        partial_stats,
        None,
        None,
    )
}

/// Converts post-publication synchronization into an achieved guarantee.
///
/// # Parameters
///
/// - `requirement`: Requested durability requirement.
/// - `sync`: One-shot synchronization operation to execute after publication.
/// - `operation`: Operation that already published its destination.
/// - `source`: Source path retained in a required-durability error.
/// - `target`: Destination path retained in a required-durability error.
///
/// # Returns
///
/// `true` when synchronization completed, or `false` for a permitted
/// preferred downgrade.
///
/// # Errors
///
/// Returns `PublicationIncomplete` when required synchronization fails after
/// the namespace mutation.
#[inline]
pub(crate) fn published_durability(
    requirement: LocalDurabilityRequirement,
    sync: impl FnOnce() -> io::Result<()>,
    operation: LocalFileOperation,
    source: &Path,
    target: &Path,
) -> LocalResult<bool> {
    match requirement {
        LocalDurabilityRequirement::NotRequired => Ok(false),
        LocalDurabilityRequirement::Preferred => Ok(sync().is_ok()),
        LocalDurabilityRequirement::Required => {
            sync().map(|()| true).map_err(|error| {
                LocalFileError::from_io(
                    operation,
                    Some(source.to_path_buf()),
                    Some(target.to_path_buf()),
                    error,
                )
                .with_kind(LocalFileErrorKind::PublicationIncomplete)
            })
        }
    }
}
