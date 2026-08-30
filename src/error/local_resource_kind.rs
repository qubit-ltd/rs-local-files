// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resource dimensions tracked by local filesystem operations.
// qubit-style: allow source-test-pair
// Covered by resource-limit walker integration tests.

use std::fmt;

/// A resource dimension enforced by local filesystem operations.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[must_use]
pub enum LocalResourceKind {
    /// Descendant depth beneath a traversal or copy root.
    Depth,
    /// A currently open native directory reader.
    OpenDirectory,
    /// A yielded or processed directory entry.
    Entry,
    /// Bytes retained by duplicate-name tracking.
    SeenNameBytes,
    /// Bytes used by one encoded native or portable path component.
    PathComponentBytes,
    /// Bytes copied by a tree-copy operation.
    CopiedBytes,
}

impl fmt::Display for LocalResourceKind {
    /// Formats the resource dimension for human-readable diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depth => formatter.write_str("depth"),
            Self::OpenDirectory => formatter.write_str("open directory"),
            Self::Entry => formatter.write_str("entry"),
            Self::SeenNameBytes => formatter.write_str("seen-name bytes"),
            Self::PathComponentBytes => formatter.write_str("path-component bytes"),
            Self::CopiedBytes => formatter.write_str("copied bytes"),
        }
    }
}
