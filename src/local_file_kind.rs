// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by metadata integration tests.

/// Normalized kind of a native filesystem entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalFileKind {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Symbolic link or Windows name-surrogate reparse point.
    Symlink,
    /// FIFO, socket, device, or another platform-specific entry.
    Other,
}
