// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Copy publication-state constructors.

use crate::{
    LocalCopyFailure,
    LocalCopyFailureState,
    LocalCopyStats,
    LocalFileError,
};

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
pub(crate) fn copy_failure_unchanged(
    error: LocalFileError,
) -> LocalCopyFailure {
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
