// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Copy conflict policy.

/// Conflict policy for existing destination file entries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalCopyConflictPolicy {
    /// Fail when a destination file entry already exists.
    #[default]
    Fail,
    /// Replace an existing destination file entry.
    Overwrite,
    /// Keep an existing destination file entry and continue copying.
    Skip,
}
