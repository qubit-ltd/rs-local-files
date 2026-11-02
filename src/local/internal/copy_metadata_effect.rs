// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Evidence about directory permission preservation performed during copy.

/// Tracks directory metadata effects separately from content counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum CopyMetadataEffect {
    /// No directory metadata operation has been applied.
    #[default]
    None,
    /// A requested directory metadata operation completed successfully.
    Applied,
    /// A native metadata operation failed with an uncertain effect.
    Indeterminate,
}
