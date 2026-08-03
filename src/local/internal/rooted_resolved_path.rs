// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private rooted-or-host path carrier.
// qubit-style: allow source-test-pair

use std::path::PathBuf;

/// Resolved path selected by a rooted operation.
#[derive(Debug)]
pub(crate) enum RootedResolvedPath {
    /// Path that remains authorized by the opened root descriptor.
    Rooted(crate::local::LocalRelativePath),
    /// Host path required after an explicitly allowed root escape.
    Host(PathBuf),
}

/// Converts a resolved rooted-or-host path into a native Host path.
///
/// # Parameters
///
/// - `root`: Opened rooted authority used for rooted descendants.
/// - `path`: Path selected by rooted resolution.
///
/// # Returns
///
/// A host path suitable for the host fallback authority.
#[inline]
pub(crate) fn resolved_host_path(
    root: &crate::rooted::Root,
    path: RootedResolvedPath,
) -> PathBuf {
    match path {
        RootedResolvedPath::Rooted(relative) => {
            root.authority_path().join(relative.as_path())
        }
        RootedResolvedPath::Host(path) => path,
    }
}
