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
    /// A currently open native directory reader.
    OpenDirectory,
}

impl fmt::Display for LocalResourceKind {
    /// Formats the resource dimension for human-readable diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenDirectory => formatter.write_str("open directory"),
        }
    }
}
