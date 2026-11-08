// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Pending work items for recursive deletion.

/// Pending work for one no-follow recursive deletion.
///
/// # Type Parameters
///
/// * `P` - The coordinate type used to identify filesystem entries.
pub(crate) enum DeleteWork<P> {
    /// Inspects an entry before deciding how to remove it.
    Inspect(
        /// Entry coordinate awaiting no-follow metadata inspection.
        P,
    ),
    /// Removes a directory after all of its children have been processed.
    RemoveDirectory(
        /// Directory coordinate whose children have already been processed.
        P,
    ),
}
