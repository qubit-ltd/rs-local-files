// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! States proven when a unified rename operation fails.

/// Namespace state proven by a failed unified rename operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalRenameFailureState {
    /// The native namespace is proven unchanged.
    Unchanged,
    /// The source was renamed to the destination before a later failure.
    Renamed,
    /// Native I/O failed without proving whether the rename took effect.
    Indeterminate,
}
