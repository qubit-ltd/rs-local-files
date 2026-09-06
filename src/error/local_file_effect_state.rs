// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by structured error integration tests.

/// Namespace effect state known from a local filesystem error.
///
/// A value of `None` from [`crate::error::LocalFileError::effect_state`] means
/// that the basic error does not provide enough evidence to classify the
/// namespace effect. It must not be interpreted as [`Self::Unchanged`].
///
/// # Examples
///
/// ```
/// use qubit_local_files::error::LocalFileEffectState;
///
/// assert_ne!(LocalFileEffectState::Unchanged, LocalFileEffectState::Applied);
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalFileEffectState {
    /// The operation did not change the namespace.
    Unchanged,
    /// The operation changed part of the namespace before failing.
    PartiallyApplied,
    /// The operation completed its namespace change before a later failure.
    Applied,
    /// The operation may have changed the namespace, but its final state is
    /// unknown.
    Indeterminate,
}
