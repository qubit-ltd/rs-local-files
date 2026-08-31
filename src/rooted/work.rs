// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deferred work items for rooted directory copying.

use super::Metadata;
use super::Path;

/// Deferred work for iterative rooted directory copying.
pub(super) enum Work {
    /// Copies the children of one directory.
    Enter {
        /// Validated source directory.
        source: Path,
        /// Validated destination directory.
        destination: Path,
        /// Source metadata applied after all children are installed.
        metadata: Metadata,
        /// Depth of this directory beneath the copied tree root.
        depth: usize,
    },
    /// Applies source permissions after a directory's children are installed.
    Finish {
        /// Source directory retained for error context.
        source: Path,
        /// Destination directory whose permissions are finalized.
        destination: Path,
        /// Source metadata supplying portable permissions.
        metadata: Metadata,
    },
}
