// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Entry states captured by bounded directory snapshots.

use std::path::PathBuf;

/// Captured state for one relative filesystem entry.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SnapshotEntry {
    /// A directory entry with no stored payload.
    Directory,
    /// A regular file and its bounded contents.
    File(Vec<u8>),
    /// A symlink and its link text, without following the target.
    Symlink(PathBuf),
}
