// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Native directory iterator and a path carried into the next batch.
pub(super) struct WorkerWalkCursor {
    /// Worker-owned native directory iterator, absent after end-of-stream.
    pub(super) walker: Option<crate::LocalDirectoryWalker>,
    /// Entry that did not fit in the previous batch's byte budget.
    pub(super) pending_entry: Option<PathBuf>,
    /// Iterator error deferred until a successful prefix is delivered.
    pub(super) pending_error: Option<std::io::Error>,
}
