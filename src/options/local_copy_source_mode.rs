// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native copy source interpretation mode.

/// Selects the source kind accepted by a unified local copy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub enum LocalCopySourceMode {
    /// Require a regular file source.
    File,
    /// Require a directory-tree source.
    Tree,
    /// Detect whether the source is a regular file or directory tree.
    #[default]
    Auto,
}
