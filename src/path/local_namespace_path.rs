// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! One normalized path in a local filesystem namespace.

use std::path::Path;
use std::path::PathBuf;

/// A path normalized against one [`crate::LocalFileSystem`] PWD snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalNamespacePath {
    /// Reusable namespace-absolute identity exposed by public values.
    namespace_absolute: PathBuf,
    /// Path representation consumed by the selected authority backend.
    authority_relative: PathBuf,
    /// Whether native input syntax requires the resolved entry to be a
    /// directory even though normalization removed the trailing syntax.
    directory_required: bool,
}

impl LocalNamespacePath {
    /// Creates one resolver-owned normalized path.
    pub(super) const fn new(
        namespace_absolute: PathBuf,
        authority_relative: PathBuf,
        directory_required: bool,
    ) -> Self {
        Self {
            namespace_absolute,
            authority_relative,
            directory_required,
        }
    }

    /// Returns the normalized namespace-absolute path.
    #[must_use]
    #[inline(always)]
    pub fn namespace_absolute(&self) -> &Path {
        &self.namespace_absolute
    }

    /// Returns the path representation consumed by the authority backend.
    ///
    /// Rooted paths omit the virtual root. Host paths remain fully qualified.
    #[must_use]
    #[inline(always)]
    pub fn authority_relative(&self) -> &Path {
        &self.authority_relative
    }

    /// Reports whether the original native syntax requires a directory.
    #[must_use]
    #[inline(always)]
    pub const fn directory_required(&self) -> bool {
        self.directory_required
    }
}
