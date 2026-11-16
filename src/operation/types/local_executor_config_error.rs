// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Invalid finite-limit configuration supplied for a local executor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalExecutorConfigError {
    /// At least one worker is required.
    ZeroWorkers,
    /// Ordinary job queue capacity must be nonzero.
    ZeroQueueCapacity,
    /// Queue waiter capacity must be nonzero.
    ZeroWaiterCapacity,
    /// Owned memory capacity must be nonzero.
    ZeroOwnedBytes,
    /// Native handle capacity must be nonzero.
    ZeroOpenHandles,
    /// Temporary disk capacity must be nonzero.
    ZeroDiskBytes,
    /// Fact snapshot capacity must be nonzero.
    ZeroFactRecords,
}
