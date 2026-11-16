// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::PathBuf;
use std::task::Waker;

/// Completion and wake state for one requested directory batch.
pub(super) struct WorkerWalkCommand {
    /// Batch result populated by the worker after the iterator advances.
    pub(super) result: Option<io::Result<Option<Vec<PathBuf>>>>,
    /// Caller task waiting for this request's result.
    pub(super) waker: Option<Waker>,
}
