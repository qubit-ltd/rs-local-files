// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private filename validation policies.

/// Filename validation policy selected by a path scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalFileNamePolicy {
    /// Conservative cross-platform UTF-8 filename rules.
    Portable,
    /// Lossless current-platform filename rules.
    Native,
}
