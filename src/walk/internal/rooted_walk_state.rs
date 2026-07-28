// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by rooted walker integration tests.

use std::sync::Arc;

use crate::rooted::Root;

use super::RootedWalkFrame;

/// Descriptor-relative state retained by a rooted directory walker.
#[derive(Debug)]
pub(in crate::walk) struct RootedWalkState {
    /// Open root authority shared with its originating filesystem object.
    pub(in crate::walk) root: Arc<Root>,
    /// Pending depth-first directory frames.
    pub(in crate::walk) stack: Vec<RootedWalkFrame>,
}
