// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Post-publication durability translation.

use std::{
    io,
    path::Path,
};

use crate::{
    LocalDurabilityRequirement,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalResult,
};

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
