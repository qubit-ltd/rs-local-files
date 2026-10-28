// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resource dimensions tracked by local filesystem operations.
// Covered by resource-limit walker integration tests.

use std::fmt;

/// A resource dimension enforced by local filesystem operations.
///
/// # Examples
///
/// ```
/// use qubit_local_files::error::LocalResourceKind;
///
/// assert_eq!(LocalResourceKind::Entry.to_string(), "entry");
/// ```
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
    /// Encoded native bytes retained by pending recursive deletion paths.
    PendingPathBytes,
    /// Bytes used by one encoded native or portable path component.
    PathComponentBytes,
    /// Bytes copied by a tree-copy operation.
    CopiedBytes,
    /// Symlink hops followed during one path resolution.
    SymlinkHops,
    /// Native bytes in one symlink target.
    SymlinkTargetBytes,
    /// Components awaiting resolution.
    PendingResolutionComponents,
    /// Components in the resolved directory prefix.
    ResolvedComponents,
    /// Native bytes in the pending resolution work set.
    PendingResolutionBytes,
    /// Native bytes in the resolved path prefix.
    ResolvedPathBytes,
    /// ACL entries decoded from a security descriptor.
    AclEntries,
    /// ACL bytes read from a security descriptor.
    AclBytes,
}

impl fmt::Display for LocalResourceKind {
    /// Formats the resource dimension for human-readable diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depth => formatter.write_str("depth"),
            Self::OpenDirectory => formatter.write_str("open directory"),
            Self::Entry => formatter.write_str("entry"),
            Self::PendingPathBytes => formatter.write_str("pending-path bytes"),
            Self::SeenNameBytes => formatter.write_str("seen-name bytes"),
            Self::PathComponentBytes => formatter.write_str("path-component bytes"),
            Self::CopiedBytes => formatter.write_str("copied bytes"),
            Self::SymlinkHops => formatter.write_str("symlink hops"),
            Self::SymlinkTargetBytes => formatter.write_str("symlink target bytes"),
            Self::PendingResolutionComponents => formatter.write_str("pending resolution components"),
            Self::ResolvedComponents => formatter.write_str("resolved components"),
            Self::PendingResolutionBytes => formatter.write_str("pending resolution bytes"),
            Self::ResolvedPathBytes => formatter.write_str("resolved path bytes"),
            Self::AclEntries => formatter.write_str("ACL entries"),
            Self::AclBytes => formatter.write_str("ACL bytes"),
        }
    }
}
