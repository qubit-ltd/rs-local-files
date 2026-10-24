// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Outcome states are covered through public copy integration tests.
//! States proven when a unified copy operation fails.

/// Publication state proven for the requested destination by a failed unified
/// copy operation.
///
/// This state describes destination entries and requested content, not
/// auxiliary parent directories created to reach the destination. With
/// `LocalCopyOptions::with_create_parent`, a failed copy can therefore report
/// [`Unchanged`](Self::Unchanged) while a newly created parent directory
/// remains. Such directories are not automatically removed because concurrent
/// operations may already be using them.
///
/// # Examples
///
/// ```
/// use qubit_local_files::outcome::LocalCopyFailureState;
///
/// let state = LocalCopyFailureState::Unchanged;
/// assert!(matches!(state, LocalCopyFailureState::Unchanged));
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalCopyFailureState {
    /// No destination entry was changed.
    Unchanged,
    /// Some destination entries were written, but the full requested content
    /// was not published.
    PartiallyPublished,
    /// The complete destination content was published before a later failure.
    Published,
    /// Native I/O failed without proving the final destination state.
    Indeterminate,
}
