// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native namespace scope exposed by a local filesystem instance.
// qubit-style: allow source-test-pair

use std::path::Path;

/// Namespace in which a [`crate::LocalFileSystem`] interprets paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalFileSystemScope<'a> {
    /// Paths use the process-visible Host namespace.
    Host,
    /// Paths are descendants of an opened root authority.
    Rooted {
        /// Non-authoritative path retained only for diagnostics.
        diagnostic_root: &'a Path,
    },
}
