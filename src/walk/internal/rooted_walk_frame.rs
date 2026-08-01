// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by rooted walker integration tests.

use std::{
    path::PathBuf,
    vec::IntoIter,
};

use crate::rooted::Entry;

/// One deferred immediate rooted directory listing in a lazy tree walk.
#[derive(Debug)]
pub(in crate::walk) struct RootedWalkFrame {
    /// Remaining immediate entries, initialized on the first iteration step.
    pub(in crate::walk) entries: Option<IntoIter<Entry>>,
    /// Authority-relative path of the listed directory.
    pub(in crate::walk) authority_parent: PathBuf,
    /// Requested-list-root-relative path of the listed directory.
    pub(in crate::walk) output_parent: PathBuf,
    /// Depth assigned to entries returned by this frame.
    pub(in crate::walk) entry_depth: usize,
}
