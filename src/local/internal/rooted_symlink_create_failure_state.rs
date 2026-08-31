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
    /// A destination placeholder remains published.
    #[cfg(windows)]
    PartiallyPublished,
    /// The final destination state could not be proven.
    #[cfg(windows)]
    Indeterminate,
}
