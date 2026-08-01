// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by host walker integration tests.

use std::{fs::ReadDir, path::PathBuf};

/// One open directory in the lazy depth-first traversal stack.
#[derive(Debug)]
pub(in crate::walk) struct WalkFrame {
    /// Native iterator for immediate entries.
    pub(in crate::walk) entries: ReadDir,
    /// Root-relative path of this directory.
    pub(in crate::walk) relative: PathBuf,
    /// Canonical identity retained while this directory is on the DFS path.
    pub(in crate::walk) identity: Option<PathBuf>,
    /// Depth assigned to entries returned by this iterator.
    pub(in crate::walk) entry_depth: usize,
}
