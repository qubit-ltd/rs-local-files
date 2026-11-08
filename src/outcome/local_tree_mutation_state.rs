// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Progress of attempted tree-content, access-metadata, or namespace changes.
///
/// This state describes mutation progress independently of publication. For
/// example, changing permissions before a no-replace installation fails can
/// yield `Started` together with
/// [`crate::outcome::LocalTreePublication::NotPublished`].
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalTreeMutationState;
///
/// let state = LocalTreeMutationState::Started;
/// assert_eq!(state, LocalTreeMutationState::Started);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTreeMutationState {
    /// No tree-content, access-metadata, or namespace change was attempted.
    NotStarted,
    /// At least one tree-content, access-metadata, or namespace change was
    /// attempted; this does not imply that publication completed.
    Started,
    /// The effects of the attempted changes could not be determined.
    Indeterminate,
}
