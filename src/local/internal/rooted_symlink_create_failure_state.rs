// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Namespace states from failed rooted symbolic-link publication.

/// Namespace state proven after rooted symbolic-link creation fails.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootedSymlinkCreateFailureState {
    /// No destination entry was created.
    Unchanged,
    #[cfg(windows)]
    /// A destination placeholder remains published.
    PartiallyPublished,
    #[cfg(windows)]
    /// The final destination state could not be proven.
    Indeterminate,
}
