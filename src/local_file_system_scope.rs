// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native namespace scope exposed by a local filesystem instance.

/// Namespace in which a [`crate::LocalFileSystem`] interprets paths.
///
/// # Examples
///
/// ```
/// use qubit_local_files::path::{LocalFileSystemScope, LocalPaths};
///
/// let paths = LocalPaths::rooted();
/// assert_eq!(paths.scope(), LocalFileSystemScope::Rooted);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalFileSystemScope {
    /// Paths use the process-visible Host namespace.
    Host,
    /// Paths are descendants of an opened root authority.
    Rooted,
}
