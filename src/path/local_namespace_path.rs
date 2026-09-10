// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! One bound path in a local filesystem namespace.

use std::path::Path;
use std::path::PathBuf;

/// A path bound to one [`crate::LocalFileSystem`] namespace.
///
/// Host operands preserve native dots and parents for filesystem traversal.
/// Rooted operands are normalized lexically against a virtual PWD snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) struct LocalNamespacePath {
    /// Reusable namespace-absolute path exposed by public values.
    namespace_absolute: PathBuf,
    /// Path representation consumed by the selected authority backend.
    authority_relative: PathBuf,
    /// Whether native input syntax requires the resolved entry to be a
    /// directory, including after Rooted lexical normalization.
    directory_required: bool,
}

impl LocalNamespacePath {
    /// Creates one resolver-owned bound path.
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

    /// Returns the namespace-absolute path, retaining Host native spelling.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) fn namespace_absolute(&self) -> &Path {
        &self.namespace_absolute
    }

    /// Returns the path representation consumed by the authority backend.
    ///
    /// Rooted paths omit the virtual root. Host paths remain fully qualified.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) fn authority_relative(&self) -> &Path {
        &self.authority_relative
    }

    /// Reports whether the original native syntax requires a directory.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) const fn directory_required(&self) -> bool {
        self.directory_required
    }
}
