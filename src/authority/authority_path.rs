// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Paths constructed and owned by namespace authorities.
// qubit-style: allow multiple-public-types

use std::path::Path;
use std::path::PathBuf;

use crate::RelativePath;

/// A validated path in the Host namespace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HostPath {
    /// A relative path interpreted through the cwd handle captured at binding.
    BoundCwd(RelativePath),
    /// An absolute native path interpreted through its native root handle.
    Absolute(PathBuf),
}

/// A path whose variant records the authority that constructed it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AuthorityPath {
    /// A Host namespace path.
    Host(HostPath),
    /// A validated path beneath an opened Rooted authority.
    Rooted(RelativePath),
}

impl AuthorityPath {
    /// Returns the non-authoritative native path retained for diagnostics.
    #[must_use]
    pub(crate) fn diagnostic_path(&self) -> &Path {
        match self {
            Self::Host(HostPath::BoundCwd(path)) | Self::Rooted(path) => path.as_path(),
            Self::Host(HostPath::Absolute(path)) => path,
        }
    }
}
