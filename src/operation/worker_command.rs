// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::task::Waker;

use super::LocalStreamCompletion;

/// Completion and wake state for one requested stream operation.
pub(super) struct WorkerCommand {
    /// Worker result not yet consumed by the polling caller.
    pub(super) result: Option<io::Result<LocalStreamCompletion>>,
    /// Caller task to wake when the command completes.
    pub(super) waker: Option<Waker>,
}
